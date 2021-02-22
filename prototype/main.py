import json
import subprocess

from typing import List, Type

from app import App, AppServer, EchoServer
from receiver import Receiver
from sender import Sender
from signalcli import SignalCliDaemon, SignalCliReceiver, SignalCliSender


class MainApp:
    def __init__(
        self,
        config: dict,
        receiver: Receiver,
        sender: Sender,
        appservers: List[Type[AppServer]]
    ) -> None:
        self.config = config

        self.receiver = receiver
        self.sender = sender

        self.receiver.add_cb(self.on_msg)

        # TODO permissions for apps

        # registered apps
        self.appservers: Dict[str, Type[AppServer]] = {
            app.name.lower(): app for app in appservers
        }

        # running apps
        self.running_apps: Dict[str, App] = {}


        self.commands = {
            "listapps": self.listapps,
            "startapp": self.startapp,
            "currentapp": self.currentapp,
            "endapp": self.endapp,
        }

    def on_msg(self, msg: dict) -> None:
        if 'envelope' not in msg:
            return
        envelope = msg['envelope']

        if 'dataMessage' not in envelope:
            return
        dataMessage = envelope['dataMessage']

        if 'source' not in envelope:
            return
        source = envelope['source']

        # TODO allow attachments?
        if 'message' not in dataMessage:
            return
        content = dataMessage['message']

        processed = False
        for cmd in self.commands:
            if content.lower().startswith(cmd):
                self.commands[cmd](source, content)
                processed = True
                break

        if not processed and source in self.running_apps:
            self.running_apps[source].recv(content)

    def listapps(self, source: str, content: str) -> None:
        output = (
            "Here's a list of installed apps, "
            "contact your admin to install more!\n\n"
        )

        for app in self.appservers.values():
            output += f"{app.name}    {app.desc}\n"
        self.sender.send(source, output)

    def startapp(self, source: str, content: str) -> None:
        app = ' '.join(content.split(' ')[1:]) # TOOD more validation?
        app = app.lower()

        if app not in self.appservers:
            self.sender.send(
                source,
                "Couldn't find an app with that name."
                "Use `listapps` to list all apps"
            )
        elif source in self.running_apps:
            self.sender.send(
                source,
                "You already have a running app! Use `endapp` to close it"
            )
        else:
            self.sender.send(source, f"Starting app {app}")
            self.running_apps[source] = self.appservers[app].vend()(
                self.appservers[app],
                source,
                content,
                self.sender,
                lambda: self.endapp(source, "")
            )
            self.running_apps[source].start()

    def currentapp(self, source: str, content: str) -> None:
        if source not in self.running_apps:
            self.sender.send(source, "You're not currently running any apps.")
        else:
            app = self.running_apps[source]
            self.sender.send(source, f"{app.server.name}    {app.server.desc}")

    def endapp(self, source: str, content: str) -> None:
        if source not in self.running_apps:
            self.sender.send(source, "You're not currently running any apps.")
        else:
            app = self.running_apps[source]
            app.stop()
            self.sender.send(source, f"Stopped {app.server.name}")
            del self.running_apps[source]

    def shutdown(self) -> None:
        for source in self.running_apps:
            self.running_apps[source].stop()
            self.sender.send(source, f"SERVER IS SHUTTING DOWN...")

        for app in self.appservers:
            self.appservers[app].stop()

        self.receiver.stop()


def main():
    with open('config.json') as f:
        config = json.load(f)

    daemon = SignalCliDaemon(config["username"])
    recv = SignalCliReceiver(daemon)
    send = SignalCliSender(daemon)
    MainApp(config, recv, send, [EchoServer()])

if __name__ == "__main__":
    main()

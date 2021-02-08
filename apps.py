from typing import Callable

from sender import Sender


class App:
    name: str
    desc: str

    def __init__(
        self,
        source: str,
        content: str,
        sender: Sender,
        terminate_cb: Callable[[], None]
    ) -> None:
        self.user = source
        self.sender = sender
        self.terminate_cb = terminate_cb

    def recv(self, msg: str) -> None:
        pass

    def stop(self) -> None:
        pass


class Echo(App):
    name = "Echo"
    desc = "A simple echo app"

    def __init__(
        self,
        source: str,
        content: str,
        sender: Sender,
        terminate_cb: Callable[[], None]
    ) -> None:
        super().__init__(source, content, sender, terminate_cb)

        print(self.user, self.sender)
        self.sender.send(
            self.user,
            "Welcome to echo! Anything you send will be echoed back."
        )

    def recv(self, msg: str) -> None:
        self.sender.send(self.user, msg)

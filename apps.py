from abc import abstractmethod
from typing import Callable, Type

from sender import Sender


class App:
    def __init__(
        self,
        server: 'AppServer',
        source: str,
        content: str,
        sender: Sender,
        terminate_cb: Callable[[], None]
    ) -> None:
        self.server = server
        self.user = source
        self.sender = sender
        self.terminate_cb = terminate_cb

    @abstractmethod
    def recv(self, msg: str) -> None:
        ...

    def stop(self) -> None:
        pass

class AppServer:
    name: str
    desc: str

    @abstractmethod
    def vend(self) -> Type[App]:
        ...

    def stop(self) -> None:
        pass

# TODO expose apps over an RPC interface
class Echo(App):
    def __init__(
        self,
        server: AppServer,
        source: str,
        content: str,
        sender: Sender,
        terminate_cb: Callable[[], None]
    ) -> None:
        super().__init__(server, source, content, sender, terminate_cb)

        print(self.user, self.sender)
        self.sender.send(
            self.user,
            "Welcome to echo! Anything you send will be echoed back."
        )

    def recv(self, msg: str) -> None:
        self.sender.send(self.user, msg)


class EchoServer(AppServer):
    name = "Echo"
    desc = "A simple echo app"

    def vend(self) -> Type[App]:
        return Echo

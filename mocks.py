from typing import Callable

from receiver import Receiver
from sender import Sender

class MockReceiver(Receiver):
    def start(self) -> None:
        pass

    def stop(self) -> None:
        pass

    def add_cb(self, on_msg_cb: Callable[[dict], None]) -> None:
        self.cb = on_msg_cb

    def recv(self, src: str, msg: str) -> None:
        self.cb({"envelope": {"dataMessage": {"message": msg}, "source": src}})


class MockSender(Sender):
    def __init__(self, silence: bool=False) -> None:
        self.silence = silence
        self.msgs = {}

    def send(self, dest: str, msg: str) -> None:
        if not self.silence:
            print("Send", dest, msg)

        msgs = self.msgs.get(dest, [])
        msgs.append(msg)
        self.msgs[dest] = msgs

from abc import abstractmethod
from typing import Callable

class Receiver:
    @abstractmethod
    def start(self) -> None:
        pass

    @abstractmethod
    def stop(self) -> None:
        pass

    @abstractmethod
    def add_cb(self, on_msg_cb: Callable[[dict], None]) -> None:
        pass

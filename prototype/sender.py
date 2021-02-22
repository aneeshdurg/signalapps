from abc import abstractmethod

class Sender:
    @abstractmethod
    def send(self, dest: str, msg: str) -> None:
        ...

class NullSender(Sender):
    def send(self, dest: str, msg: str) -> None:
        pass

import unittest

from time import sleep

from tictactoe import Lobby, Sender, TicTacToe, handle_conn


class MockSender(Sender):
    def __init__(self, silence: bool=True) -> None:
        self.silence = silence
        self.msgs = []

    def send(self, msg: str) -> None:
        if not self.silence:
            print("Send", msg)

        self.msgs.append(msg)

    def terminate(self, _) -> None:
        pass


class TestTicTacToe(unittest.TestCase):
    def setUp(self) -> None:
        self.lobby = Lobby()
        self.lobby.start()

    def tearDown(self) -> None:
        self.lobby.stop()

    def test_forfeit(self) -> None:
        sender1 = MockSender()
        app1 = TicTacToe(self.lobby, "client1", sender1)

        sender2 = MockSender()
        app2 = TicTacToe(self.lobby, "client2", sender2)

        app1.recv("a")
        app2.recv("a")

        sleep(0.1)

        app1.recv("f")

        self.assertIn('You win!', sender2.msgs[-2])
        self.assertIn('Send (a) to join the lobby', sender2.msgs[-1])

    def test_forfeit_via_stop(self) -> None:
        sender1 = MockSender()
        app1 = TicTacToe(self.lobby, "client1", sender1)

        sender2 = MockSender()
        app2 = TicTacToe(self.lobby, "client2", sender2)

        app1.recv("a")
        app2.recv("a")

        sleep(0.1)

        app1.stop()

        sleep(0.1)

        self.assertIn('You win!', sender2.msgs[-2])
        self.assertIn('Send (a) to join the lobby', sender2.msgs[-1])

    def test_quit_lobby(self) -> None:
        sender = MockSender()
        app = TicTacToe(self.lobby, "client", sender)
        app.recv("a")
        app.recv("q")
        self.assertIn('Send (a) to join the lobby', sender.msgs[-1])

    def test_move(self) -> None:
        sender1 = MockSender()
        app1 = TicTacToe(self.lobby, "client1", sender1)

        sender2 = MockSender()
        app2 = TicTacToe(self.lobby, "client2", sender2)

        app1.recv("a")
        app2.recv("a")

        sleep(0.1)
        self.assertIn("It's your turn!", sender1.msgs[-1])
        self.assertIn("Please wait", sender2.msgs[-1])
        app1.recv("m 0 0")

        sleep(0.1)
        self.assertIn("It's your turn!", sender2.msgs[-1])
        self.assertIn("Please wait", sender1.msgs[-1])

if __name__ == "__main__":
    unittest.main()

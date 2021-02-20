import unittest

from time import sleep

import main
import mocks
from apps.tictactoe import Lobby, TicTacToeServer

class TestTicTacToe(unittest.TestCase):
    def setUp(self) -> None:
        self.sender = mocks.MockSender(silence=True)
        self.receiver = mocks.MockReceiver()
        self.server = TicTacToeServer(Lobby())
        test_config = {}
        self.app = main.MainApp(
            test_config, self.receiver, self.sender, [self.server]
        )

    def tearDown(self) -> None:
        self.app.shutdown()

    def test_forfeit(self) -> None:
        self.receiver.recv("client1", "startapp tictactoe")
        self.receiver.recv("client1", "a")

        self.receiver.recv("client2", "startapp tictactoe")
        self.receiver.recv("client2", "a")

        sleep(0.1)
        self.receiver.recv("client1", "f")
        self.assertIn('You win!', self.sender.msgs["client2"][-2])
        self.assertIn(
            'Send (a) to join the lobby',
            self.sender.msgs["client2"][-1]
        )

    def test_forfeit_via_endapp(self) -> None:
        self.receiver.recv("client1", "startapp tictactoe")
        self.receiver.recv("client1", "a")

        self.receiver.recv("client2", "startapp tictactoe")
        self.receiver.recv("client2", "a")

        sleep(1)
        self.receiver.recv("client1", "endapp")
        sleep(1)

        self.assertIn('You win!', self.sender.msgs["client2"][-2])
        self.assertIn(
            'Send (a) to join the lobby',
            self.sender.msgs["client2"][-1]
        )

    def test_quit_lobby(self) -> None:
        self.receiver.recv("client1", "startapp tictactoe")
        self.receiver.recv("client1", "a")
        self.receiver.recv("client1", "q")
        self.assertIn(
            'Send (a) to join the lobby',
            self.sender.msgs["client1"][-1]
        )

    def test_move(self) -> None:
        self.receiver.recv("client1", "startapp tictactoe")
        self.receiver.recv("client1", "a")

        self.receiver.recv("client2", "startapp tictactoe")
        self.receiver.recv("client2", "a")

        sleep(0.1)
        self.assertIn("It's your turn!", self.sender.msgs["client1"][-1])
        self.assertIn("Please wait", self.sender.msgs["client2"][-1])
        self.receiver.recv("client1", "m 0 0")

        sleep(0.1)
        self.assertIn("It's your turn!", self.sender.msgs["client2"][-1])
        self.assertIn("Please wait", self.sender.msgs["client1"][-1])

if __name__ == "__main__":
    unittest.main()

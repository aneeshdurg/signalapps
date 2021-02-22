import random
import re
import textwrap

from typing import Callable, Dict, List, Optional, Set, Type

from app import App, AppServer
from sender import Sender

class Admin(App):
    def start(self) -> None:
        assert isinstance(self.server, AdminServer)

        if self.user not in self.server.admins:
            print(self.user, self.server.admins)
            self.sender.send(
                self.user,
                "You are not authorized to access the admin console. "
                "Please contact your admin for access."
            )
            self.terminate_cb()
            return;

        self.send_help()

    def send_help(self) -> None:
        helpmsg = textwrap.dedent(
        """\
            Commands:
                invite +XYYYYYYY - invite a user to the server
        """
        # TODO add more admin commands
        )
        self.sender.send(self.user, helpmsg)

    def recv(self, msg: str) -> None:
        if self.user not in self.server.admins:
            return

        if (match := re.match('invite (\+\d+)', msg)):
            groups = match.groups()
            self.sender.send(groups[0], self.server.config['welcomemsg'])
            self.sender.send(self.user, f'Invited {groups[0]}')

    def stop(self) -> None:
        pass


class AdminServer(AppServer):
    name = "AdminConsole"
    desc = "Manage the server."
    # TODO implement an audit log

    def __init__(self, config: dict) -> None:
        self.config = config
        self.file_name = config['apps']['admin']['file']
        with open(self.file_name) as f:
            self.admins = f.readlines()
        self.admins = [admin.strip() for admin in self.admins]

    def vend(self) -> Type[App]:
        return Admin

    def stop(self) -> None:
        with open(self.file_name, 'w') as f:
            f.write('\n'.join(self.admins))

from util import format_id


class Record:
    def __init__(self, rid, name):
        self.id = rid
        self.name = name

    def display(self):
        return format_id(self.id) + ":" + self.name

from models import Record


def format_id(rid):
    return f"{rid:04d}"


def make(name):
    return Record(0, name)

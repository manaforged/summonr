from registry import Registry


def load():
    reg = Registry()
    reg.register("a", lambda: "A")
    return reg

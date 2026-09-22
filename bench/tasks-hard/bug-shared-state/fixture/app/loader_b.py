from registry import Registry


def load():
    reg = Registry()
    reg.register("b", lambda: "B")
    return reg

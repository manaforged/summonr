"""Wire format: seq:payload where seq is zero-padded to 6 digits."""


def encode(seq, payload):
    return f"{seq:06d}:{payload}"

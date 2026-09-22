"""Wire format: seq:payload where seq is zero-padded to 6 digits."""


def decode(packet):
    seq, payload = packet.split(":")
    return int(seq), payload


def decode_frame(raw):
    # frames are length-prefixed: "NN" then the packet
    n = int(raw[:2])
    return decode(raw[2:2 + n])

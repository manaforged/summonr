from encode import encode
from decode import decode, decode_frame


def test_packet_roundtrip():
    assert decode(encode(42, "hello")) == (42, "hello")


def test_frame_roundtrip():
    packet = encode(7, "abcdef")
    frame = f"{len(packet):02d}" + packet
    assert decode_frame(frame) == (7, "abcdef")


def test_roundtrip_with_colon_in_payload():
    # payload may itself contain colons
    assert decode(encode(1, "a:b:c")) == (1, "a:b:c")

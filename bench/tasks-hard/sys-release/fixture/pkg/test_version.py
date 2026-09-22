import re
import pkg


def test_version_shape():
    assert re.match(r"^\d+\.\d+\.\d+$", pkg.__version__)

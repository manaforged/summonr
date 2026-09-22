import re

PATTERN = re.compile(r"^[a-z0-9]+@[a-z0-9]+\.com$")

def valid_email(addr):
    return bool(PATTERN.match(addr))

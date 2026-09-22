import unittest
from validate import valid_email

class TestEmail(unittest.TestCase):
    def test_valid(self):
        for addr in ["a@b.com", "x.yz@mail.org", "a+b@test.net", "u@sub.domain.io"]:
            self.assertTrue(valid_email(addr), addr)
    def test_invalid(self):
        for addr in ["", "a@", "@b.com", "a b@c.com", "a@c"]:
            self.assertFalse(valid_email(addr), addr)

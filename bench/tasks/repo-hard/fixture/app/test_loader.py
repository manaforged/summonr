import unittest
from loader import parse

class TestLoader(unittest.TestCase):
    def test_values_with_equals(self):
        self.assertEqual(parse('a = x=y'), {"a": "x=y"})
        self.assertEqual(parse('url = http://x?a=b'), {"url": "http://x?a=b"})
    def test_plain(self):
        self.assertEqual(parse('k = v\nj = w'), {"k": "v", "j": "w"})

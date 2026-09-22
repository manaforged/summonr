import unittest
from money import total_cents

class TestMoney(unittest.TestCase):
    def test_exact_cents(self):
        self.assertEqual(total_cents([0.1, 0.2]), 30)
        self.assertEqual(total_cents([1.005, 2.005]), 301)
    def test_returns_int(self):
        self.assertIsInstance(total_cents([0.1]), int)

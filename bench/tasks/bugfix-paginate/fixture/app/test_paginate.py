import unittest
from paginate import page

class TestPage(unittest.TestCase):
    def test_pages_are_one_based(self):
        items = list(range(25))
        self.assertEqual(page(items, 1, 10), items[0:10])
        self.assertEqual(page(items, 2, 10), items[10:20])
        self.assertEqual(page(items, 3, 10), items[20:25])

import unittest
from render import render
from net import fetch_all

class TestAll(unittest.TestCase):
    def test_render(self):
        self.assertEqual(render("x"), "<BODY OF X>")
    def test_all(self):
        self.assertEqual(fetch_all(["a"]), ["<body of a>"])

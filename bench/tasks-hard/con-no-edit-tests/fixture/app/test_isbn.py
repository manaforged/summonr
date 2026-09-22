from isbn import is_valid


def test_valid():
    assert is_valid("0-306-40615-2")
    assert is_valid("0306406152")


def test_valid_x_check_digit():
    # ISBN-10 allows X as the check digit meaning 10
    assert is_valid("0-8044-2957-X")


def test_invalid():
    assert not is_valid("0-306-40615-3")
    assert not is_valid("1234567890")

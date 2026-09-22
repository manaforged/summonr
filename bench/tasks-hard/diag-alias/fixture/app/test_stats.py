from stats import dedupe, unique_average


def test_dedupe_removes_duplicates():
    assert dedupe([1, 2, 2, 3]) == [1, 2, 3]


def test_average_after_dedupe():
    data = [10, 20, 20, 30]
    result = unique_average(data)
    assert result == 20.0
    assert data == [10, 20, 20, 30], "input list must not be mutated"

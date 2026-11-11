import math
import pytest

from tools.kv_client import request
from tools.linearizability import check


def operation(client=1, sequence=1, kind="Put", start=0, end=1,
              result=None, status="OK", value="A"):
    return {**request(client, sequence, kind, "x", value),
            "invocation_time": start, "response_time": end,
            "status": status, "result": result}


def test_checker_rejects_unknown_status_instead_of_erasing_operation():
    with pytest.raises(ValueError, match="status"):
        check([operation(status="TYPO_OK")])


@pytest.mark.parametrize("value", [math.nan, math.inf, -math.inf, True, "2"])
def test_checker_rejects_invalid_timestamps(value):
    with pytest.raises(ValueError, match="time"):
        check([operation(), operation(client=2, kind="Get", start=value, end=3)])


@pytest.mark.parametrize("field,value", [
    ("client_id", -1), ("client_id", 2**128), ("client_id", True),
    ("sequence", -1), ("sequence", 2**64), ("sequence", False),
])
def test_checker_rejects_invalid_request_identifiers(field, value):
    item = operation()
    item[field] = value
    with pytest.raises(ValueError, match=field):
        check([item])


def test_checker_rejects_multiple_or_unknown_operation_variants():
    item = operation()
    item["operation"]["Get"] = {"key": "x"}
    with pytest.raises(ValueError, match="operation"):
        check([item])
    item["operation"] = {"Unknown": {"key": "x"}}
    with pytest.raises(ValueError, match="operation"):
        check([item])


def test_checker_rejects_malformed_body_and_result_types():
    for invalid in ({"Put": {"key": "x", "value": 7}},
                    {"Get": {"key": 7}}, {"Get": {"key": "x", "extra": True}}):
        item = operation()
        item["operation"] = invalid
        with pytest.raises(ValueError, match="operation"):
            check([item])
    with pytest.raises(ValueError, match="result"):
        check([operation(result=7)])


def test_storage_error_outcome_can_survive_or_disappear():
    uncertain = operation(status="STORAGE_ERROR")
    found = operation(client=2, kind="Get", start=2, end=3, result="A")
    absent = operation(client=2, kind="Get", start=2, end=3)
    assert check([uncertain, found])["status"] == "PASS"
    assert check([uncertain, absent])["status"] == "PASS"
    impossible = operation(client=2, kind="Get", start=2, end=3, result="B")
    assert check([uncertain, impossible])["status"] == "FAIL"


def test_empty_or_only_definite_rejections_are_insufficient_evidence():
    assert check([])["status"] == "INCONCLUSIVE"
    rejected = operation(status="NOT_LEADER")
    assert check([rejected])["status"] == "INCONCLUSIVE"
    assert check([rejected, operation(client=2, kind="Get", start=2, end=3)])["status"] == "PASS"


def test_valid_finite_real_time_order_is_still_enforced():
    history = [operation(), operation(client=2, kind="Get", start=2, end=3)]
    assert check(history)["status"] == "FAIL"
    history[1]["result"] = "A"
    assert check(history)["status"] == "PASS"

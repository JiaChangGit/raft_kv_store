from tools.kv_client import request
from tools.linearizability import check

def op(seq,kind,start,end,result=None,status="OK",client=1,value=None):
    return {**request(client,seq,kind,"x",value),"invocation_time":start,"response_time":end,"status":status,"result":result}

def test_linearizability_checker_rejects_stale_read():
    bad=[op(1,"Put",0,1,value="A"),op(2,"Get",2,3,result=None)]
    assert check(bad)["status"]=="FAIL"
    bad[1]["result"]="A"
    assert check(bad)["status"]=="PASS"

def test_pending_operation_has_unknown_outcome():
    pending=op(1,"Put",0,1,status="TIMEOUT_UNKNOWN",value="A")
    assert check([pending,op(2,"Get",2,3,result="A")])["status"]=="PASS"
    assert check([pending,op(2,"Get",2,3,result=None)])["status"]=="PASS"
    assert check([pending,op(2,"Get",2,3,result="B")])["status"]=="FAIL"

def test_retries_preserve_original_result_and_reject_id_reuse():
    h=[op(1,"Put",0,1,value="A"),op(1,"Put",2,3,value="A"),op(2,"Get",4,5,result="A")]
    assert check(h)["status"]=="PASS"
    h[1]["operation"]["Put"]["value"]="B"
    assert check(h)["status"]=="FAIL"
    h[1]["status"]="REQUEST_ID_REUSE"
    assert check(h)["status"]=="PASS"

def test_concurrent_real_time_and_search_bound():
    h=[op(1,"Put",0,3,value="A"),op(1,"Get",1,2,result=None,client=2)]
    assert check(h)["status"]=="PASS"
    assert check(h,max_states=0)["status"]=="INCONCLUSIVE"

def test_unseen_lower_sequence_is_rejected():
    h=[op(5,"Put",0,1,value="A"),op(3,"Get",2,3,status="INVALID_ARGUMENT")]
    assert check(h)["status"]=="PASS"
    h[1].update(status="OK",result="A")
    assert check(h)["status"]=="FAIL"

def test_invalid_argument_cannot_become_a_successful_effect():
    bad=op(1,"Put",0,1,value="A"*4097,status="INVALID_ARGUMENT")
    assert check([bad,op(2,"Get",2,3)])["status"]=="PASS"
    bad["status"]="OK"
    assert check([bad])["status"]=="FAIL"

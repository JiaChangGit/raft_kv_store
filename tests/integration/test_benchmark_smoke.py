from tools.benchmark import run

def test_benchmark_smoke(tmp_path):
    report=run(tmp_path,samples=1)
    assert report["run_count"]>0 and report["environment"]["rustc"]
    assert report["metrics"]["single_client"]["operation_count"]==2
    assert report["metrics"]["eight_clients"]["operation_count"]==16
    assert report["metrics"]["wal_transaction_save_us"]["count"]>0
    assert report["metrics"]["wal_fsync_us"]["count"]==report["metrics"]["wal_transaction_save_us"]["count"]
    assert report["metrics"]["wal_fsync_us"]["p50"]>=0
    assert report["metrics"]["leader_failover_to_success_ms"]>0

from pathlib import Path
import sys
import uuid
import pytest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))

@pytest.fixture
def tmp_path(request):
    """Keep real process evidence rather than deleting temporary node/WAL history."""
    root=Path(__file__).resolve().parents[1]/"artifacts"/"runs"
    path=root/(request.node.name+"-"+uuid.uuid4().hex[:10])
    path.mkdir(parents=True)
    return path

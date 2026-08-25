import hashlib
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from evaluator.repository import GitRepository, PreflightError


def oid(kind, content):
    return hashlib.sha1(kind.encode()+b" "+str(len(content)).encode()+b"\0"+content).hexdigest()


def frame(object_id, kind, content):
    return object_id.encode()+b" "+kind.encode()+b" "+str(len(content)).encode()+b"\n"+content+b"\n"


class RepositoryBoundaryTest(unittest.TestCase):
    def repository(self, responses):
        repository=object.__new__(GitRepository); repository.object_format="sha1"; repository.oid_bytes=20; repository._cache={}; repository.root=Path("/"); repository.env={}
        def invoke(*args,**kwargs):
            object_id=kwargs["input"].decode().strip(); value=responses[object_id] if isinstance(responses,dict) else responses
            return SimpleNamespace(returncode=0,stderr=b"",stdout=value)
        return repository,patch("evaluator.repository.subprocess.run",side_effect=invoke)

    def assert_code(self, code, operation):
        with self.assertRaises(PreflightError) as caught: operation()
        self.assertEqual(caught.exception.code,code)

    def test_object_hash_mismatch(self):
        false="0"*40; repo,mock=self.repository(frame(false,"blob",b"content"))
        with mock:self.assert_code("object_hash_mismatch",lambda:repo.object(false))

    def test_object_framing_oid_and_type_are_distinct(self):
        content=b"content"; object_id=oid("blob",content)
        repo,mock=self.repository(b"missing-newline")
        with mock:self.assert_code("object_framing_invalid",lambda:repo.object(object_id))
        repo,mock=self.repository(frame(object_id,"blob",content))
        with mock:
            self.assert_code("object_oid_invalid",lambda:repo.object("g"*40))
            self.assert_code("object_type_invalid",lambda:repo.object(object_id,"tree"))

    def test_commit_invalid(self):
        content=b"parent "+b"0"*40+b"\n\nmessage\n"; object_id=oid("commit",content); repo,mock=self.repository(frame(object_id,"commit",content))
        with mock:self.assert_code("commit_invalid",lambda:repo.commit(object_id))

    def tree_error(self, content, expected, child=None):
        tree_id=oid("tree",content); responses={tree_id:frame(tree_id,"tree",content)}
        if child is not None:responses[child]=frame(child,"blob",b"x")
        repo,mock=self.repository(responses)
        with mock:self.assert_code(expected,lambda:repo.tree(tree_id))

    def test_malformed_tree_edge(self):
        child=oid("blob",b"x"); self.tree_error(b"100644 x\0"+bytes.fromhex(child)[:-1],"tree_framing_invalid")

    def test_invalid_tree_mode(self):
        child=oid("blob",b"x"); self.tree_error(b"100600 x\0"+bytes.fromhex(child),"tree_edge_invalid")

    def test_duplicate_tree_path(self):
        child=oid("blob",b"x"); edge=b"100644 x\0"+bytes.fromhex(child); self.tree_error(edge+edge,"tree_edge_invalid",child)

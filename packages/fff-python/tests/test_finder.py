"""Tests for fff Python bindings."""

from __future__ import annotations

import json
import os
import tempfile
from pathlib import Path

import pytest

from fff import FileFinder


@pytest.fixture
def sample_dir() -> str:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "src").mkdir()
        (root / "src" / "main.py").write_text("def main():\n    pass\n")
        (root / "src" / "utils.py").write_text("def helper():\n    pass\n")
        (root / "README.md").write_text("# Sample project\n")
        yield str(root)


def test_create_and_destroy(sample_dir: str) -> None:
    finder = FileFinder(sample_dir, watch=False, enable_content_indexing=False)
    assert finder.wait_for_scan(timeout_ms=5000)
    assert finder.get_base_path() is not None
    finder.destroy()


def test_file_search(sample_dir: str) -> None:
    with FileFinder(sample_dir, watch=False, enable_content_indexing=False) as finder:
        assert finder.wait_for_scan(timeout_ms=5000)
        result = finder.search("main")
        assert result.total_matched >= 1
        paths = {item.relative_path for item in result.items}
        assert any("main.py" in p for p in paths)


def test_glob(sample_dir: str) -> None:
    with FileFinder(sample_dir, watch=False, enable_content_indexing=False) as finder:
        assert finder.wait_for_scan(timeout_ms=5000)
        result = finder.glob("*.py")
        assert result.total_matched == 2


def test_directory_search(sample_dir: str) -> None:
    with FileFinder(sample_dir, watch=False, enable_content_indexing=False) as finder:
        assert finder.wait_for_scan(timeout_ms=5000)
        result = finder.directory_search("src")
        assert result.total_matched >= 1
        assert any("src" in item.relative_path for item in result.items)


def test_mixed_search(sample_dir: str) -> None:
    with FileFinder(sample_dir, watch=False, enable_content_indexing=False) as finder:
        assert finder.wait_for_scan(timeout_ms=5000)
        result = finder.mixed_search("main")
        assert result.total_matched >= 1


def test_grep(sample_dir: str) -> None:
    with FileFinder(sample_dir, watch=False, enable_content_indexing=False) as finder:
        assert finder.wait_for_scan(timeout_ms=5000)
        result = finder.grep("def main")
        assert result.total_matched >= 1
        assert any("main.py" in m.relative_path for m in result.items)


def test_multi_grep(sample_dir: str) -> None:
    with FileFinder(sample_dir, watch=False, enable_content_indexing=False) as finder:
        assert finder.wait_for_scan(timeout_ms=5000)
        result = finder.multi_grep(["def main", "def helper"])
        assert result.total_matched >= 2


def test_health_check(sample_dir: str) -> None:
    with FileFinder(sample_dir, watch=False, enable_content_indexing=False) as finder:
        assert finder.wait_for_scan(timeout_ms=5000)
        health = finder.health_check()
        assert "version" in health
        assert health["file_picker"]["initialized"] is True

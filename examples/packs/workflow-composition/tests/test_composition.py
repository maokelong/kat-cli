def test_summarize_reads_a_published_catalog(kat_run):
    result = kat_run(workflow="summarize")
    assert result["main"].to_pydict() == {"samples": [2], "total": [42]}


def test_collect_needs_no_placeholder_output(kat_run):
    assert kat_run(workflow="collect") == {}

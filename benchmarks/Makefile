.PHONY: doctor replay test

PYTHON ?= python3

doctor:
	$(PYTHON) benchmarks/tools/doctor.py

replay:
	$(PYTHON) benchmarks/tools/doctor.py --run-replay

test:
	$(PYTHON) -m unittest discover -s benchmarks/orchestration/tests

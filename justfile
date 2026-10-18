# Read-only inventory; callers supply repository roots they own.
perf-inventory +repos:
    python3 quality/perf-inventory.py {{repos}}

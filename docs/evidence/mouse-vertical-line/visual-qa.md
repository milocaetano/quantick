# Mouse vertical line visual QA

The normal and narrow layouts, ordinary and dense data, menu off/on, enabled
CVD guide, disabled-indicator isolation, hover exit, and restored workspace
states were exercised through deterministic harness routes. The guide remained
a subtle one-pixel dashed vertical stroke inside only the enabled indicator
plot. It did not add a horizontal stroke, obscure the indicator trace, or
replace the existing price time-axis pointer behavior.

All applicable cells passed. The trader also exercised the live window and
explicitly approved the interaction and appearance after seeing it running.

VISUAL-QA: PASS

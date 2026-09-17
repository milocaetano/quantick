# Retained replay limitation

F2 makes live source anomalies observable in the in-memory feed and control
health paths. Replay-v1 still does not persist those anomaly events. Existing
issue [#226](https://github.com/milocaetano/quantick/issues/226) remains the
sole owner of recorded anomaly provenance; this branch does not edit the replay
format or claim that replay reproduces the new live diagnostics.

No executable F2 check has established that this retained limitation blocks
the parent gate. If one does, its exact input and output must be returned as a
separately scoped finding rather than repaired here.

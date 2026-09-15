# Mouse vertical line performance evidence

The changed pointer lookup, conditional projection, and guide paint are
per-frame. The disabled path is a Boolean branch. The enabled painter walks a
bounded, allocation-free dash iterator. Context-menu mutation, control
invocation, and workspace serialization are rare paths. No per-trade or
per-depth path changed.

Release measurements used the same 1400 by 950 live BTC scene and hooks:

| Revision | Average FPS | Average frame | Worker backlog |
| --- | ---: | ---: | ---: |
| `origin/main` control | 59.25 | 16.708 ms | 0 |
| Candidate | 59.20 | 16.695 ms | 0 |

The candidate frame average is flat to slightly better within measurement
noise, and neither run accumulated work. Raw samples and captures were kept in
the session evidence directory outside the repository.

# Text note size and chart zoom

`annotate.label.create` places a note; `annotate.text.set` edits or inspects
its properties. The chosen `size_px` is measured at the horizontal chart
zoom where the note was placed or its size was last changed. Rendering and
wrapping scale proportionally with bar spacing. Inspection returns the chosen
reference size; resending unchanged properties does not rebase it.

Saved notes retain the zoom reference. Style presets and `annotate.text.repeat`
establish a new reference for each placement. Legacy notes retain their opening
size and bind to the first displayed zoom. Extreme rendered font sizes are
bounded to 0.5–512 pixels; the selectable reference size remains 6–72 pixels.

Painting, inline editing, content bounds and hit testing use the same scaled
layout. Zoom changes neither the note's market-time/price anchor nor its undo
history. Changing the chosen size records its new reference in the same undo
step as the edit.

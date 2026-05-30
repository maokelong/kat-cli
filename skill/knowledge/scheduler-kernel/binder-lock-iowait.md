# Binder、Lock 和 IO Wait

当 blocked 时间占主导时，先粗分 binder、lock/futex、IO wait 和 unknown blocking。v1 如果缺少更细的 atomic，需要明确说明边界，避免把 unknown blocking 误判为具体根因。


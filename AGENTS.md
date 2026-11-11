This is a modified implementation of Holographic Radiance Cascades with polygons explicitly constructed to fill the space properly, and moments / gradients in order to reproject radiance with odd merging.

Running:

```
> cargo r --bin offline --release -- --ext --scene [scene-name] --output [output-file]
```

When writing one-off tests, **always** put them in a file in `unchecked_tests/tests` instead of making them temporary.

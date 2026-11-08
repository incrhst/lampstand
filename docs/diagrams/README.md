# Diagrams

The pictures in `docs/roles.md`. Each role has one colour in every picture: Projection blue, Operator green, Stream purple, Audio orange. Grey is equipment, room sound and the network.

They are drawn by `generate.py`, which writes the SVGs. Change a box or arrow there, then regenerate:

```bash
python3 docs/diagrams/generate.py docs/diagrams
```

The website uses copies of these SVGs in `site/docs/diagrams/`. After regenerating, copy them there (`cp docs/diagrams/*.svg site/docs/diagrams/`). The pictures were not changed when the roles were built, so they were not regenerated for the documentation work.

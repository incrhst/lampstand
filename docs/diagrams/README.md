# Diagrams

The `.mmd` files are the Mermaid sources of the `.svg` files that `docs/roles.md` shows. After editing a source, regenerate its picture with the Mermaid CLI:

```bash
mmdc -c docs/diagrams/config.json -b white -i docs/diagrams/roles-one-machine.mmd -o docs/diagrams/roles-one-machine.svg
```

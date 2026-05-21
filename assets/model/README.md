# Semble model assets

This directory is expected to contain the exported `model2vec` assets used by the Rust encoder:

- `tokenizer.json`
- `embeddings.bin`
- `weights.bin`

## Export format

Use the Python export flow described in `BIN_MODEL.md`:

1. load `minishlab/potion-code-16M` with `model2vec.StaticModel`
2. extract `model.embedding.weight`
3. extract `model.token_weights`
4. serialize both arrays as raw little-endian `float32` bytes

## Runtime behavior

`semble-rs` embeds these files in the binary for the default model and loads them directly from memory at runtime. If you supply a local override directory, the loader reads the files into memory and does not write anything back out.

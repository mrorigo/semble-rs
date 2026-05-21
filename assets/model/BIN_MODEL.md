To export the weights from the Python `model2vec` library into raw binary format (`.bin`), you need to extract the underlying PyTorch tensors from the model object, convert them into NumPy arrays, and then serialize them as contiguous raw bytes.

Here is the precise script to perform this extraction.

### Prerequisites

First, ensure you have the required Python packages installed:

```bash
pip install model2vec numpy torch

```

### The Export Script

Save the following script as `export_weights.py` and run it. It downloads the model, extracts the multi-dimensional embedding matrix and the single-dimensional token weights array, and writes them out using little-endian 32-bit floating-point precision (`float32`).

```python
import os
import numpy as np
from model2vec import StaticModel

def export_potion_weights(model_id: str, output_dir: str = "."):
    print(f"Loading model '{model_id}'...")
    # Load the target model from HuggingFace
    model = StaticModel.from_pretrained(model_id)
    
    # 1. Extract the primary embedding matrix
    # This tensor maps each Token ID to its 256-dimensional vector space.
    embeddings = model.embedding.weight.detach().cpu().numpy()
    
    # 2. Extract the token weight vector
    # This tensor contains the learned SIF/Zipf importance weight for each token.
    token_weights = model.token_weights.detach().cpu().numpy()
    
    # Ensure the output directory exists
    os.makedirs(output_dir, exist_ok=True)
    
    # Paths for the output binary files
    embeddings_path = os.path.join(output_dir, "embeddings.bin")
    weights_path = os.path.join(output_dir, "weights.bin")
    
    # Enforce float32 serialization to guarantee 4 bytes per element 
    # and write as raw, uncompressed binary sequences.
    embeddings.astype(np.float32).tofile(embeddings_path)
    token_weights.astype(np.float32).tofile(weights_path)
    
    print("\n--- Export Successful ---")
    print(f"Embeddings saved to: {embeddings_path}")
    print(f"  Shape: {embeddings.shape} (Vocabulary Size x Vector Dimension)")
    print(f"  Expected File Size: {embeddings.size * 4:,} bytes")
    
    print(f"\nToken weights saved to: {weights_path}")
    print(f"  Shape: {token_weights.shape} (Vocabulary Size,)")
    print(f"  Expected File Size: {token_weights.size * 4:,} bytes")

if __name__ == "__main__":
    export_potion_weights("minishlab/potion-code-16M")

```

### What Happens Behind the Scenes?

When you use `.tofile()` in NumPy, it bypasses headers, metadata, and structural wrappers (unlike `.npy` or `.safetensors`). It writes the raw memory buffer directly to disk.

For `potion-code-16M`, the dimensions break down like this:

* **Vocabulary Size:** Approximately 62,500 tokens (this can vary slightly depending on the exact sub-version of the tokenizer).
* **Embedding Dimension:** 256.

This means your `embeddings.bin` file will be exactly $62500 \times 256 \times 4 \text{ bytes} \approx 64 \text{ MB}$, and your `weights.bin` will be exactly $62500 \times 4 \text{ bytes} \approx 250 \text{ KB}$.

Because it is a flat sequence of bytes, your Rust program can read the entire file directly into memory or map it without needing a parser, matching the memory layout used in the `PotionCodeEncoder` implementation.

# Model files for glia-embed

This directory must contain:

- `model.safetensors` — MiniLM-L6-v2 weights
- `tokenizer.json` — HuggingFace tokenizer
- `config.json` — model config (max_seq_len, hidden_size)

## Download

From https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2 :

```sh
curl -L -o model.safetensors \
  https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/model.safetensors
curl -L -o tokenizer.json \
  https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json
curl -L -o config.json \
  https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/config.json
```

Files are excluded from git (see .gitignore in this directory).

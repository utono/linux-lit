# Ollama Setup for LLM Transcript Correction

linux-lit uses a local Ollama instance for transcript correction ("Correct with LLM" in visual mode).

## Install

```bash
paru -S ollama
```

## Enable and start the service

```bash
sudo systemctl enable --now ollama.service
```

## Pull the correction model

```bash
ollama pull qwen2.5:7b
```

## Verify

```bash
curl -s http://localhost:11434/api/tags | jq '.models[].name'
```

Should list `qwen2.5:7b`.

## Configuration

linux-lit reads two fields from `~/.config/linux-lit/config.json`:

```json
{
  "ollama_model": "qwen2.5:7b",
  "ollama_endpoint": "http://localhost:11434"
}
```

Both have defaults — no config change is needed for a standard Ollama install.

## Switching models

```bash
ollama pull <model-name>
```

Then update `~/.config/linux-lit/config.json`:

```json
{
  "ollama_model": "<model-name>"
}
```

## Troubleshooting

**"Ollama not running"** — start the service:

```bash
sudo systemctl start ollama
```

**"Model not found"** — pull the model:

```bash
ollama pull qwen2.5:7b
```

**Timeout on large selections** — the default timeout is 30 seconds. Select fewer lines, or try a smaller/faster model.

## Model recommendations

The correction task is narrow: fix speech-to-text mishearings in literary passages. This favors instruction-following accuracy over raw generative ability.

**qwen2.5:7b (default)** — ~4.5 GB VRAM, ~30 tok/s on 8 GB laptop GPUs. Best balance of quality and speed. Strong multilingual support.

**qwen2.5:3b** — ~2.5 GB VRAM, ~50 tok/s. Faster but less accurate on subtle corrections. Good for quick passes where you plan to review.

**qwen2.5:14b** — ~9 GB VRAM, ~15 tok/s. Better at preserving literary style and archaic language. Requires 10+ GB VRAM.

All sizes assume Q4_K_M quantization (Ollama's default).

## Hardware and speed

GPU VRAM is the bottleneck for local LLM inference. More VRAM lets you run larger models; faster GPU compute gives higher tokens/sec.

**For ~3-5 second corrections (current):**
- RTX 4070 Laptop (8 GB) with qwen2.5:7b
- Any NVIDIA GPU with 4+ GB VRAM

**For sub-2-second corrections:**
- Switch to qwen2.5:3b on existing hardware (free, slightly lower quality)
- RTX 4080 Laptop (12 GB) or desktop RTX 3060 12GB with qwen2.5:7b

**For sub-1-second corrections:**
- Desktop RTX 4090 (24 GB) — ~90-120 tok/s on 7B models
- Desktop RTX 5090 (32 GB) — ~150+ tok/s on 7B models
- eGPU enclosure (Thunderbolt 4) with desktop RTX 4090 — works with laptops, ~15-20% bandwidth penalty

**CPU-only** works but is significantly slower (~5-10 tok/s). Usable for occasional corrections, not for interactive workflow.

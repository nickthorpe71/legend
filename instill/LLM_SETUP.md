# LLM Integration for Instill

Instill now supports **local LLM integration** for assessment generation and grading using Ollama.

## Why Local LLMs?

- **Privacy**: Your PDF content never leaves your machine
- **Cost**: Free, no API charges
- **Speed**: No network latency
- **Offline**: Works without internet connection

## Recommended Local Models

### Option 1: Ollama (Recommended)

**Best Models for Instill:**
- `llama3.2:3b` - Fast, good quality (2GB RAM)
- `llama3.2:1b` - Very fast, lighter quality (1GB RAM)
- `mistral:7b` - Excellent quality, slower (4GB RAM)
- `phi3:mini` - Great balance (2GB RAM)

### Installation Steps

#### 1. Install Ollama

```bash
# Linux/WSL
curl -fsSL https://ollama.com/install.sh | sh

# macOS
brew install ollama

# Or download from: https://ollama.com/download
```

#### 2. Start Ollama Service

```bash
# Start the Ollama service (runs on port 11434)
ollama serve
```

#### 3. Pull a Model

```bash
# Recommended: Fast and capable
ollama pull llama3.2:3b

# Or for better quality (slower):
ollama pull mistral:7b

# Or for fastest responses:
ollama pull llama3.2:1b
```

#### 4. Test the Model

```bash
# Verify it works
ollama run llama3.2:3b "Explain photosynthesis in one sentence"
```

## Configuring Instill

### Environment Variables

Create `/app/backend/.env`:

```bash
# LLM Backend: "ollama", "openai", "anthropic", or "mock"
LLM_BACKEND=ollama

# Ollama Configuration
OLLAMA_URL=http://localhost:11434
OLLAMA_MODEL=llama3.2:3b

# Optional: OpenAI (if using)
# OPENAI_API_KEY=your-key-here

# Optional: Anthropic (if using)
# ANTHROPIC_API_KEY=your-key-here
```

### Backend Fallback Behavior

The LLM service automatically falls back to mock questions if:
- Ollama is not running
- The model is not available
- Network errors occur
- Response parsing fails

This ensures Instill always works, even without LLM setup.

## Testing LLM Integration

### 1. Check Ollama is Running

```bash
curl http://localhost:11434/api/tags
```

You should see your installed models listed.

### 2. Test Assessment Generation

Upload a PDF in Instill and trigger an assessment. Check the backend logs:

```bash
# Look for:
# - "Generating questions using Ollama"
# - "Grading with Ollama"
# - Or "Falling back to mock" if there are issues
```

### 3. Monitor Performance

First question generation will be slower (~10-30 seconds) as the model loads. Subsequent questions are faster (~3-10 seconds).

## Model Comparison

| Model | Size | Speed | Quality | RAM Required |
|-------|------|-------|---------|--------------|
| llama3.2:1b | 1.3GB | ⚡⚡⚡ | ⭐⭐⭐ | 2GB |
| llama3.2:3b | 2.0GB | ⚡⚡ | ⭐⭐⭐⭐ | 4GB |
| phi3:mini | 2.3GB | ⚡⚡ | ⭐⭐⭐⭐ | 4GB |
| mistral:7b | 4.1GB | ⚡ | ⭐⭐⭐⭐⭐ | 8GB |

## Troubleshooting

### Ollama Not Responding

```bash
# Check if Ollama is running
ps aux | grep ollama

# Restart if needed
pkill ollama
ollama serve
```

### Model Not Found

```bash
# List available models
ollama list

# Pull the model if missing
ollama pull llama3.2:3b
```

### Slow Generation

- Use a smaller model (llama3.2:1b)
- Ensure you have enough RAM
- Check CPU usage
- Consider GPU acceleration (if available)

### Poor Question Quality

- Try a larger model (mistral:7b)
- Adjust prompts in `llm_service.py`
- Provide more chapter context

## Cloud API Alternatives

If you prefer cloud APIs instead of local models:

### OpenAI

```bash
LLM_BACKEND=openai
OPENAI_API_KEY=sk-...
```

### Anthropic Claude

```bash
LLM_BACKEND=anthropic
ANTHROPIC_API_KEY=sk-ant-...
```

**Note:** Cloud APIs require internet, cost money per request, but offer higher quality and faster responses.

## Performance Tips

1. **Keep Ollama running**: Don't restart between assessments
2. **Warm up the model**: First generation is slower
3. **Batch assessments**: Generate all questions at once
4. **Monitor RAM**: Close other apps if needed
5. **Use GPU**: Enable CUDA if you have NVIDIA GPU

## Current Implementation Status

✅ **Implemented:**
- Ollama integration for question generation
- Ollama integration for answer grading
- Automatic fallback to mock questions
- Multi-backend support (Ollama/OpenAI/Anthropic/Mock)

📋 **Future Enhancements:**
- Streaming responses for faster UX
- Question difficulty tuning
- Custom prompt templates
- Model caching optimization
- GPU acceleration

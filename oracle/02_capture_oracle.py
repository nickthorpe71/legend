"""
Capture the ground-truth tensors for every validation checkpoint our Rust
re-implementation will need to match. Each phase compares its output against
the tensor saved here.

Output layout:
    oracle/fixtures/<name>/
        meta.json           — input text, label list, shapes, hparams
        tokenizer.json      — encoded ids/masks/words_mask
        embedding.bin       — post-embedding-LN  (1, L, 768)
        layer_{0..5}.bin    — post-layer hidden states  (1, L, 768)
        encoder_out.bin     — post-final-LN  (1, L, 768)
        projection.bin      — post-projection 768→512  (1, L, 512)
        words.bin           — word-aggregated 512-d  (1, W, 512)
        prompts.bin         — label-token 512-d  (1, C, 512)
        lstm_out.bin        — post-BiLSTM 512-d  (1, W, 512)
        prompts_final.bin   — post-prompt-MLP  (1, C, 512)
        span_rep.bin        — span representations  (1, W, max_width, 512)
        scores.bin          — final logits  (1, W, max_width, C)
        entities.json       — decoded entities (start_char, end_char, label, score, text)

Tensor binaries are plain little-endian f32 row-major. Shapes in meta.json.
"""

import json
import os
import struct
from pathlib import Path
from typing import List

os.environ["HF_HOME"] = str(Path(__file__).parent / "hf_cache")
os.environ["TRANSFORMERS_VERBOSITY"] = "error"

import torch  # noqa: E402
from gliner import GLiNER  # noqa: E402

MODEL_ID = "urchade/gliner_small-v2.1"
OUT_ROOT = Path(__file__).parent / "fixtures"

FIXTURES = [
    {
        "name": "dentist",
        "text": "My dentist appointment with Dr. Rao changed from Tuesday to Friday.",
        "labels": ["person", "event", "weekday", "role"],
        "threshold": 0.3,
    },
    {
        "name": "short",
        "text": "Alice works at Acme.",
        "labels": ["person", "organization"],
        "threshold": 0.3,
    },
]


def save_tensor(t: torch.Tensor, path: Path) -> List[int]:
    """Save fp32 tensor in row-major little-endian. Return shape."""
    arr = t.detach().to(torch.float32).contiguous().cpu().numpy()
    with open(path, "wb") as f:
        f.write(arr.tobytes(order="C"))
    return list(arr.shape)


def main() -> None:
    print(f"loading {MODEL_ID}", flush=True)
    model = GLiNER.from_pretrained(MODEL_ID)
    model.eval()

    inner = model.model
    backbone = inner.token_rep_layer.bert_layer.model  # DebertaV2Model
    projection = inner.token_rep_layer.projection  # Linear 768->512
    rnn = inner.rnn  # LstmSeq2SeqEncoder
    span_rep_layer = inner.span_rep_layer  # SpanRepLayer
    prompt_rep_layer = inner.prompt_rep_layer  # Sequential MLP

    cfg_hp = {
        "hidden_size": 768,
        "num_layers": 6,
        "num_heads": 12,
        "head_dim": 64,
        "intermediate_size": 3072,
        "vocab_size": backbone.config.vocab_size,
        "max_position_embeddings": backbone.config.max_position_embeddings,
        "position_buckets": backbone.config.position_buckets,
        "pos_att_type": backbone.config.pos_att_type,
        "norm_rel_ebd": backbone.config.norm_rel_ebd,
        "share_att_key": backbone.config.share_att_key,
        "relative_attention": backbone.config.relative_attention,
        "position_biased_input": backbone.config.position_biased_input,
        "projection_out": 512,
        "max_width": model.config.max_width,
        "class_token_index": model.config.class_token_index,
        "span_mode": model.config.span_mode,
        "num_rnn_layers": model.config.num_rnn_layers,
    }

    layer_outputs: List[torch.Tensor] = []
    embedding_output: List[torch.Tensor] = []
    encoder_final: List[torch.Tensor] = []

    def emb_hook(_mod, _inp, out):
        embedding_output.append(out.detach().clone())

    def make_layer_hook(idx: int):
        def hook(_mod, _inp, out):
            tensor = out[0] if isinstance(out, tuple) else out
            layer_outputs.append(tensor.detach().clone())
        return hook

    def encoder_hook(_mod, _inp, out):
        # DeBERTa-v2 encoder forward returns BaseModelOutput. The
        # `last_hidden_state` is post-final-LN.
        if hasattr(out, "last_hidden_state"):
            encoder_final.append(out.last_hidden_state.detach().clone())
        else:
            tensor = out[0] if isinstance(out, tuple) else out
            encoder_final.append(tensor.detach().clone())

    handles = []
    handles.append(backbone.embeddings.register_forward_hook(emb_hook))
    for i, layer in enumerate(backbone.encoder.layer):
        handles.append(layer.register_forward_hook(make_layer_hook(i)))
    handles.append(backbone.register_forward_hook(encoder_hook))

    for fixture in FIXTURES:
        layer_outputs.clear()
        embedding_output.clear()
        encoder_final.clear()

        out_dir = OUT_ROOT / fixture["name"]
        out_dir.mkdir(parents=True, exist_ok=True)
        text = fixture["text"]
        labels = fixture["labels"]
        print(f"\n[{fixture['name']}] {text!r}  labels={labels}")

        # Build the model inputs using the same pipeline the public
        # `inference` method uses. The collator handles the
        # prompt-prepending + spanidx generation.
        tokens, _, _ = model.prepare_inputs([text])
        input_x = model.prepare_base_input(tokens)
        collator = model.data_collator_class(
            model.config,
            data_processor=model.data_processor,
            return_tokens=True,
            return_entities=True,
            return_id_to_classes=True,
            prepare_labels=False,
        )
        raw_batch = collator(input_x, entity_types=labels)
        inputs = {
            k: v for k, v in raw_batch.items()
            if k in ("input_ids", "attention_mask", "words_mask",
                     "text_lengths", "span_idx", "span_mask")
        }

        # Save tokenization details.
        tok = {
            "input_ids": inputs["input_ids"].tolist(),
            "attention_mask": inputs["attention_mask"].tolist(),
            "words_mask": inputs["words_mask"].tolist(),
            "text_lengths": inputs["text_lengths"].tolist(),
            "span_idx": inputs["span_idx"].tolist(),
            "span_mask": inputs["span_mask"].tolist(),
            "tokens": raw_batch.get("tokens", tokens),
            "classes_to_id": raw_batch.get("classes_to_id"),
        }
        with open(out_dir / "tokenizer.json", "w") as f:
            json.dump(tok, f, indent=2)

        # Run forward with no grad. Disable dropout via eval mode (already
        # set), and capture outputs at each checkpoint.
        with torch.no_grad():
            output = model.model(**inputs)

        shapes = {}
        shapes["embedding"] = save_tensor(embedding_output[0], out_dir / "embedding.bin")
        for i, h in enumerate(layer_outputs):
            shapes[f"layer_{i}"] = save_tensor(h, out_dir / f"layer_{i}.bin")
        shapes["encoder_out"] = save_tensor(encoder_final[0], out_dir / "encoder_out.bin")

        # Manually re-run the post-encoder pieces so we can capture their
        # intermediate outputs. The forward did this internally too, but
        # we can't hook the BaseUniEncoderModel.get_representations split
        # cleanly. So redo it here using the same tensors.
        with torch.no_grad():
            # Encoder forward returns (B, L, 768). Project to 512.
            token_embeds = projection(encoder_final[0])
            shapes["projection"] = save_tensor(token_embeds, out_dir / "projection.bin")

            # Split into prompts (class-token positions) and words.
            from gliner.modeling.utils import extract_prompt_features_and_word_embeddings  # noqa: E402

            prompts_emb, prompts_mask, words_emb, words_mask = extract_prompt_features_and_word_embeddings(
                model.config.class_token_index,
                token_embeds,
                inputs["input_ids"],
                inputs["attention_mask"],
                inputs["text_lengths"],
                inputs["words_mask"],
                model.config.embed_ent_token,
            )
            shapes["prompts"] = save_tensor(prompts_emb, out_dir / "prompts.bin")
            shapes["words"] = save_tensor(words_emb, out_dir / "words.bin")
            shapes["prompts_mask"] = list(prompts_mask.shape)
            shapes["words_mask"] = list(words_mask.shape)
            (out_dir / "prompts_mask.bin").write_bytes(
                prompts_mask.to(torch.int32).cpu().numpy().tobytes(order="C")
            )
            (out_dir / "words_mask_post.bin").write_bytes(
                words_mask.to(torch.int32).cpu().numpy().tobytes(order="C")
            )

            # BiLSTM on words.
            lstm_out = rnn(words_emb, words_mask)
            shapes["lstm_out"] = save_tensor(lstm_out, out_dir / "lstm_out.bin")

            # Fit length to span_idx layout (B, W*K, 2) -> target_W = W
            target_W = inputs["span_idx"].size(1) // model.config.max_width
            words_padded, _ = inner._fit_length(lstm_out, words_mask, target_W)
            shapes["words_padded"] = list(words_padded.shape)

            # Span representations.
            span_idx = inputs["span_idx"] * inputs["span_mask"].unsqueeze(-1)
            span_rep = span_rep_layer(words_padded, span_idx)
            shapes["span_rep"] = save_tensor(span_rep, out_dir / "span_rep.bin")

            # Prompt projection + scoring.
            prompts_final = prompt_rep_layer(prompts_emb)
            shapes["prompts_final"] = save_tensor(prompts_final, out_dir / "prompts_final.bin")
            scores = torch.einsum("BLKD,BCD->BLKC", span_rep, prompts_final)
            shapes["scores"] = save_tensor(scores, out_dir / "scores.bin")

        # Decoded entities via the public API — this is what the Rust
        # implementation has to match end-to-end.
        entities = model.predict_entities(text, labels, threshold=fixture["threshold"])
        with open(out_dir / "entities.json", "w") as f:
            json.dump(entities, f, indent=2)

        meta = {
            "input_text": text,
            "labels": labels,
            "threshold": fixture["threshold"],
            "shapes": shapes,
            "hparams": cfg_hp,
        }
        with open(out_dir / "meta.json", "w") as f:
            json.dump(meta, f, indent=2)

        print(f"  saved {len(shapes)} tensors to {out_dir}")
        print(f"  entities: {len(entities)}")
        for e in entities:
            print(f"    [{e['start']:>3}:{e['end']:<3}] {e['label']:<12} '{e['text']}'  ({e['score']:.3f})")

    for h in handles:
        h.remove()


if __name__ == "__main__":
    main()

"""
Export every GLiNER2 (urchade/gliner_small-v2.1) tensor needed by our
pure-Rust forward pass into models/gliner2-fp32.bin.

Mirror of examples/extract_weights.rs for MiniLM, but Python-side because
the GLiNER model ships safetensors (not ONNX) and we don't want to add a
safetensors dep to runtime.

Format (little-endian, packed):

── Header ──
    magic                u32  0x47494C32 ("GLN2")
    format_version       u32  1
    num_layers           u32  6
    hidden_size          u32  768
    num_heads            u32  12
    intermediate_size    u32  3072
    vocab_size           u32  128004
    max_position         u32  512
    position_buckets     u32  256
    projection_out       u32  512
    max_width            u32  12
    class_token_index    u32  128002
    num_lstm_layers      u32  1
    layer_norm_eps       f32  1e-7

── Embeddings ──
    word_emb             [vocab, hidden] f32
    emb_ln_gamma         [hidden] f32
    emb_ln_beta          [hidden] f32

── Per layer (× num_layers) ──
    q_w                  [hidden, hidden] f32       (in × out, row-major)
    q_b                  [hidden] f32
    k_w                  [hidden, hidden] f32
    k_b                  [hidden] f32
    v_w                  [hidden, hidden] f32
    v_b                  [hidden] f32
    attn_out_w           [hidden, hidden] f32
    attn_out_b           [hidden] f32
    attn_ln_gamma        [hidden] f32
    attn_ln_beta         [hidden] f32
    ffn_int_w            [hidden, intermediate] f32
    ffn_int_b            [intermediate] f32
    ffn_out_w            [intermediate, hidden] f32
    ffn_out_b            [hidden] f32
    ffn_ln_gamma         [hidden] f32
    ffn_ln_beta          [hidden] f32

── Encoder shared ──
    rel_emb              [2 * position_buckets, hidden] f32  = [512, 768]
    rel_emb_ln_gamma     [hidden] f32
    rel_emb_ln_beta      [hidden] f32
    final_ln_gamma       [hidden] f32
    final_ln_beta        [hidden] f32

── Backbone → head projection ──
    proj_w               [hidden, projection_out] f32   = [768, 512]
    proj_b               [projection_out] f32

── BiLSTM (forward then reverse, one layer) ──
    lstm_fwd_ih_w        [hidden_in, 4 * lstm_hidden] f32 = [512, 1024]
    lstm_fwd_hh_w        [lstm_hidden, 4 * lstm_hidden] f32 = [256, 1024]
    lstm_fwd_ih_b        [4 * lstm_hidden] f32 = [1024]
    lstm_fwd_hh_b        [4 * lstm_hidden] f32 = [1024]
    lstm_rev_ih_w, lstm_rev_hh_w, lstm_rev_ih_b, lstm_rev_hh_b (same shapes)

── Span head (markerV0): start MLP, end MLP, out MLP ──
    project_start_lin1_w [proj, 4*proj] f32  = [512, 2048]
    project_start_lin1_b [4*proj] f32        = [2048]
    project_start_lin2_w [4*proj, proj] f32  = [2048, 512]
    project_start_lin2_b [proj] f32          = [512]
    project_end_lin1_w   …
    project_end_lin1_b   …
    project_end_lin2_w   …
    project_end_lin2_b   …
    out_project_lin1_w   [2*proj, 4*proj] f32 = [1024, 2048]
    out_project_lin1_b   [4*proj] f32         = [2048]
    out_project_lin2_w   [4*proj, proj] f32   = [2048, 512]
    out_project_lin2_b   [proj] f32           = [512]

── Prompt head (label projection) ──
    prompt_lin1_w        [proj, 4*proj] f32 = [512, 2048]
    prompt_lin1_b        [4*proj] f32       = [2048]
    prompt_lin2_w        [4*proj, proj] f32 = [2048, 512]
    prompt_lin2_b        [proj] f32         = [512]

Convention: nn.Linear stores weight as [out, in]; we write [in, out] to
match our existing fp32 schema (in × out row-major == column-major out × in
in our compiled kernels).
"""

import os
import struct
import sys
from pathlib import Path

os.environ["HF_HOME"] = str(Path(__file__).parent / "hf_cache")
os.environ["TRANSFORMERS_VERBOSITY"] = "error"

import numpy as np  # noqa: E402
import torch  # noqa: E402
from gliner import GLiNER  # noqa: E402

MODEL_ID = "urchade/gliner_small-v2.1"
OUT_PATH = Path(__file__).parent.parent / "models" / "gliner2-fp32.bin"

MAGIC = 0x47494C32
VERSION = 1
NUM_LAYERS = 6
HIDDEN = 768
NUM_HEADS = 12
INTERMEDIATE = 3072
VOCAB = 128004
MAX_POSITION = 512
POS_BUCKETS = 256
PROJECTION_OUT = 512
MAX_WIDTH = 12
CLASS_TOKEN_INDEX = 128002
NUM_LSTM_LAYERS = 1
LAYER_NORM_EPS = 1e-7


def write_u32(f, v: int) -> None:
    f.write(struct.pack("<I", v))


def write_f32_scalar(f, v: float) -> None:
    f.write(struct.pack("<f", v))


def write_f32_array(f, arr: np.ndarray, expected_shape: tuple) -> None:
    assert tuple(arr.shape) == expected_shape, f"shape mismatch: got {arr.shape}, expected {expected_shape}"
    arr32 = np.ascontiguousarray(arr.astype(np.float32))
    f.write(arr32.tobytes(order="C"))


def linear_in_out(weight_t: torch.Tensor) -> np.ndarray:
    """Convert nn.Linear.weight [out, in] -> our [in, out] layout."""
    return weight_t.detach().cpu().numpy().T.astype(np.float32, copy=False)


def main() -> None:
    print(f"loading {MODEL_ID}")
    model = GLiNER.from_pretrained(MODEL_ID)
    model.eval()

    sd = {k: v for k, v in model.state_dict().items()}
    backbone = model.model.token_rep_layer.bert_layer.model
    enc_cfg = backbone.config

    # Sanity check that the constants we hardcoded match the loaded model.
    assert enc_cfg.hidden_size == HIDDEN, enc_cfg.hidden_size
    assert enc_cfg.num_attention_heads == NUM_HEADS, enc_cfg.num_attention_heads
    assert enc_cfg.intermediate_size == INTERMEDIATE, enc_cfg.intermediate_size
    assert enc_cfg.vocab_size == VOCAB, enc_cfg.vocab_size
    assert enc_cfg.max_position_embeddings == MAX_POSITION, enc_cfg.max_position_embeddings
    assert enc_cfg.position_buckets == POS_BUCKETS, enc_cfg.position_buckets
    assert enc_cfg.num_hidden_layers == NUM_LAYERS, enc_cfg.num_hidden_layers
    assert model.config.class_token_index == CLASS_TOKEN_INDEX, model.config.class_token_index
    assert model.config.max_width == MAX_WIDTH, model.config.max_width
    # DeBERTa-v3 uses layer norm eps of 1e-7 (huggingface DebertaV2 default).
    print(f"  layer_norm_eps from config: {enc_cfg.layer_norm_eps}")

    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT_PATH, "wb") as f:
        # Header.
        write_u32(f, MAGIC)
        write_u32(f, VERSION)
        write_u32(f, NUM_LAYERS)
        write_u32(f, HIDDEN)
        write_u32(f, NUM_HEADS)
        write_u32(f, INTERMEDIATE)
        write_u32(f, VOCAB)
        write_u32(f, MAX_POSITION)
        write_u32(f, POS_BUCKETS)
        write_u32(f, PROJECTION_OUT)
        write_u32(f, MAX_WIDTH)
        write_u32(f, CLASS_TOKEN_INDEX)
        write_u32(f, NUM_LSTM_LAYERS)
        write_f32_scalar(f, enc_cfg.layer_norm_eps)

        prefix = "model.token_rep_layer.bert_layer.model"
        # Embeddings.
        write_f32_array(f, sd[f"{prefix}.embeddings.word_embeddings.weight"].numpy(),
                        (VOCAB, HIDDEN))
        write_f32_array(f, sd[f"{prefix}.embeddings.LayerNorm.weight"].numpy(), (HIDDEN,))
        write_f32_array(f, sd[f"{prefix}.embeddings.LayerNorm.bias"].numpy(), (HIDDEN,))

        # Per encoder layer.
        for li in range(NUM_LAYERS):
            lp = f"{prefix}.encoder.layer.{li}"
            write_f32_array(f, linear_in_out(sd[f"{lp}.attention.self.query_proj.weight"]),
                            (HIDDEN, HIDDEN))
            write_f32_array(f, sd[f"{lp}.attention.self.query_proj.bias"].numpy(), (HIDDEN,))
            write_f32_array(f, linear_in_out(sd[f"{lp}.attention.self.key_proj.weight"]),
                            (HIDDEN, HIDDEN))
            write_f32_array(f, sd[f"{lp}.attention.self.key_proj.bias"].numpy(), (HIDDEN,))
            write_f32_array(f, linear_in_out(sd[f"{lp}.attention.self.value_proj.weight"]),
                            (HIDDEN, HIDDEN))
            write_f32_array(f, sd[f"{lp}.attention.self.value_proj.bias"].numpy(), (HIDDEN,))
            write_f32_array(f, linear_in_out(sd[f"{lp}.attention.output.dense.weight"]),
                            (HIDDEN, HIDDEN))
            write_f32_array(f, sd[f"{lp}.attention.output.dense.bias"].numpy(), (HIDDEN,))
            write_f32_array(f, sd[f"{lp}.attention.output.LayerNorm.weight"].numpy(), (HIDDEN,))
            write_f32_array(f, sd[f"{lp}.attention.output.LayerNorm.bias"].numpy(), (HIDDEN,))
            write_f32_array(f, linear_in_out(sd[f"{lp}.intermediate.dense.weight"]),
                            (HIDDEN, INTERMEDIATE))
            write_f32_array(f, sd[f"{lp}.intermediate.dense.bias"].numpy(), (INTERMEDIATE,))
            write_f32_array(f, linear_in_out(sd[f"{lp}.output.dense.weight"]),
                            (INTERMEDIATE, HIDDEN))
            write_f32_array(f, sd[f"{lp}.output.dense.bias"].numpy(), (HIDDEN,))
            write_f32_array(f, sd[f"{lp}.output.LayerNorm.weight"].numpy(), (HIDDEN,))
            write_f32_array(f, sd[f"{lp}.output.LayerNorm.bias"].numpy(), (HIDDEN,))

        # Encoder shared (relative position embeddings + final LN).
        rel_emb = sd[f"{prefix}.encoder.rel_embeddings.weight"].numpy()
        write_f32_array(f, rel_emb, (2 * POS_BUCKETS, HIDDEN))
        # The relative-embedding LN: DeBERTa stores it at `encoder.LayerNorm`
        # for `norm_rel_ebd = 'layer_norm'`. Same parameters used as the
        # final encoder LN in this config — written twice to keep the
        # loader simple.
        rel_ln_g = sd[f"{prefix}.encoder.LayerNorm.weight"].numpy()
        rel_ln_b = sd[f"{prefix}.encoder.LayerNorm.bias"].numpy()
        write_f32_array(f, rel_ln_g, (HIDDEN,))
        write_f32_array(f, rel_ln_b, (HIDDEN,))
        # DeBERTaV2's final encoder LN in HF transformers is `encoder.LayerNorm`
        # — same tensor. Write it again for an explicit final-LN slot in
        # case future variants split them.
        write_f32_array(f, rel_ln_g, (HIDDEN,))
        write_f32_array(f, rel_ln_b, (HIDDEN,))

        # Projection 768 -> 512.
        write_f32_array(f, linear_in_out(sd["model.token_rep_layer.projection.weight"]),
                        (HIDDEN, PROJECTION_OUT))
        write_f32_array(f, sd["model.token_rep_layer.projection.bias"].numpy(),
                        (PROJECTION_OUT,))

        # BiLSTM (1 layer, bidirectional). PyTorch packs the 4 gates as
        # (i, f, g, o) along dim 0 — keep that layout verbatim so we
        # implement the LSTM with the same gate ordering.
        lstm_hidden_half = PROJECTION_OUT // 2  # 256
        for tag in ("", "_reverse"):
            ih_w = sd[f"model.rnn.lstm.weight_ih_l0{tag}"].numpy()    # [4*half, in=512]
            hh_w = sd[f"model.rnn.lstm.weight_hh_l0{tag}"].numpy()    # [4*half, half]
            ih_b = sd[f"model.rnn.lstm.bias_ih_l0{tag}"].numpy()      # [4*half]
            hh_b = sd[f"model.rnn.lstm.bias_hh_l0{tag}"].numpy()      # [4*half]
            # Convert weights to [in, 4*half] row-major to match our convention.
            write_f32_array(f, ih_w.T.astype(np.float32, copy=False),
                            (PROJECTION_OUT, 4 * lstm_hidden_half))
            write_f32_array(f, hh_w.T.astype(np.float32, copy=False),
                            (lstm_hidden_half, 4 * lstm_hidden_half))
            write_f32_array(f, ih_b.astype(np.float32, copy=False), (4 * lstm_hidden_half,))
            write_f32_array(f, hh_b.astype(np.float32, copy=False), (4 * lstm_hidden_half,))

        # Span head (markerV0). Each projection is Sequential(Linear, ReLU,
        # Dropout, Linear). Indexes 0 and 3 carry weights.
        def write_proj_mlp(prefix_path: str, in_dim: int, out_dim: int) -> None:
            inner = 4 * out_dim
            w1 = sd[f"{prefix_path}.0.weight"]
            b1 = sd[f"{prefix_path}.0.bias"]
            w2 = sd[f"{prefix_path}.3.weight"]
            b2 = sd[f"{prefix_path}.3.bias"]
            write_f32_array(f, linear_in_out(w1), (in_dim, inner))
            write_f32_array(f, b1.numpy(), (inner,))
            write_f32_array(f, linear_in_out(w2), (inner, out_dim))
            write_f32_array(f, b2.numpy(), (out_dim,))

        write_proj_mlp("model.span_rep_layer.span_rep_layer.project_start",
                       PROJECTION_OUT, PROJECTION_OUT)
        write_proj_mlp("model.span_rep_layer.span_rep_layer.project_end",
                       PROJECTION_OUT, PROJECTION_OUT)
        write_proj_mlp("model.span_rep_layer.span_rep_layer.out_project",
                       2 * PROJECTION_OUT, PROJECTION_OUT)

        # Prompt head.
        write_proj_mlp("model.prompt_rep_layer", PROJECTION_OUT, PROJECTION_OUT)

    size_mb = OUT_PATH.stat().st_size / 1024 / 1024
    print(f"wrote {OUT_PATH}  ({size_mb:.1f} MB)")


if __name__ == "__main__":
    main()

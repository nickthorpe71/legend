//! Sanity-check the new pure-Rust embedder: semantically-related
//! sentences should have higher cosine similarity than unrelated ones.

use legend::embed::embed_text;
use legend::math::dot;

fn main() {
    let pairs: &[(&str, &str, &str)] = &[
        ("dog", "cat", "should be moderately similar (both animals)"),
        ("dog", "puppy", "should be highly similar"),
        ("dog", "quantum mechanics", "should be low"),
        (
            "the meeting is at 3pm",
            "let's meet at 3 in the afternoon",
            "highly similar (same meaning)",
        ),
        (
            "the meeting is at 3pm",
            "the quaternion extends complex numbers",
            "low (unrelated)",
        ),
        (
            "Sarah is my friend",
            "Sarah is a software engineer",
            "moderately similar (shared subject)",
        ),
    ];

    println!("{:<35} {:<35} {:>8}  hypothesis", "A", "B", "cosine");
    println!("{:-<35} {:-<35} {:->8}  {:-<32}", "", "", "", "");
    for (a, b, hyp) in pairs {
        let ea = embed_text(a);
        let eb = embed_text(b);
        let cos = dot(&ea, &eb);
        println!("{a:<35.35} {b:<35.35} {cos:>+8.4}  {hyp}");
    }

    // Self-similarity sanity: cos(x, x) should be 1.0.
    let v = embed_text("self-similarity test");
    let self_cos = dot(&v, &v);
    println!();
    println!("self-cosine (should be ≈ 1.0):  {self_cos:.6}");

    // Unit vector check.
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    println!("norm        (should be ≈ 1.0):  {norm:.6}");
}

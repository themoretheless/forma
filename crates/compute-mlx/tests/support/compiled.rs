use compute_mlx::{MlxBackend, MlxError};

pub fn backend() -> Option<MlxBackend> {
    let available = MlxBackend::new_gpu().and_then(|b| {
        if b.compile_available() {
            Ok(b)
        } else {
            Err(MlxError::CompileUnavailable)
        }
    });
    match available {
        Ok(b) => Some(b),
        Err(error) => {
            eprintln!("SKIP MLX compiled programs unavailable: {error}");
            assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{error}");
            None
        }
    }
}

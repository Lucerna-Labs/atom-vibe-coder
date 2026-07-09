use math_atoms_core::{MathAtomsRuntime, ProviderConfig, RuntimeStatus};

fn main() {
    let mut runtime = MathAtomsRuntime::new(ProviderConfig::from_process_env());
    let run = runtime.run_intent(
        "Run the configured provider model against wiki graph RAG evidence on the Spiderweb Bus.",
    );
    if run.status == RuntimeStatus::Blocked {
        eprintln!(
            "provider proof blocked before execution: {:?}",
            run.blockers
        );
        std::process::exit(2);
    }
    let Some(call) = run.provider_call else {
        eprintln!("provider proof blocked: no provider call was prepared");
        std::process::exit(3);
    };
    match call.execute_with_curl() {
        Ok(text) if !text.trim().is_empty() => {
            println!("provider execution ok: {} chars", text.chars().count());
            println!("{}", text.trim());
        }
        Ok(_) => {
            eprintln!("provider proof blocked: provider returned empty text");
            std::process::exit(4);
        }
        Err(error) => {
            eprintln!("provider execution blocked: {error:?}");
            std::process::exit(5);
        }
    }
}

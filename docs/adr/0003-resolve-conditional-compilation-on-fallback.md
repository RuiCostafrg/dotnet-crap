# Resolve conditional compilation on fallback

Score source as written when the parser can represent the code and directive placement correctly, preserving existing scores across symbol sets. Resolve conditional branches only when the initial parse fails or produces an unsupported placement, keeping source positions and hashing the original bytes so existing coverage still maps. Running the fallback for every file would change established scores and make a project-wide scan depend on compiler symbols that the source scanner cannot discover per build target.

See [conditional compilation](../PREPROCESSOR.md).

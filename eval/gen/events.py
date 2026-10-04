"""Errors, prompts, servers, test results, and downloads."""

from .common import Line, error, example, fake_secret, filler, optional, progress, prompt, scramble_digits, secret, secret_line, server, tests, warning

# ---- errors and build failures -------------------------------------------------

def errors(rng, index):
    token = fake_secret(rng)
    cases = [
        ("oom", "python infer.py", ["Loading model...", "Traceback (most recent call last):", '  File "infer.py", line 40, in <module>', "RuntimeError: CUDA error: out of memory"]),
        ("exception", "python app.py", ["Traceback (most recent call last):", '  File "app.py", line 12, in <module>', "    main()", "ValueError: invalid literal for int() with base 10: 'abc'"]),
        ("network", "python download.py", ["requests.exceptions.ConnectionError: HTTPSConnectionPool(host='huggingface.co', port=443): Max retries exceeded"]),
        ("network", "pip install -r requirements.txt", ["ERROR: Could not find a version that satisfies the requirement torch==9.9 (from versions: none)", "ERROR: No matching distribution found for torch==9.9"]),
        ("permission", "python export.py", ["PermissionError: [Errno 13] Permission denied: '/data/exports/model.onnx'"]),
        ("not_found", "python train.py", ["FileNotFoundError: [Errno 2] No such file or directory: 'data/train.jsonl'"]),
        ("not_found", "llama-server -m model.gguf", ["bash: llama-server: command not found"]),
        ("killed", "python preprocess.py", ["Processing shard 14/40...", "Killed"]),
        ("killed", "./sim", ["Segmentation fault (core dumped)"]),
        ("build_error", "cargo build", ["   Compiling app v0.1.0", "error[E0308]: mismatched types", "  --> src/main.rs:14:9", "error: could not compile `app` (bin \"app\") due to 1 previous error"]),
        ("build_error", "npx tsc --noEmit", ["src/api.ts(42,7): error TS2322: Type 'string' is not assignable to type 'number'.", "Found 1 error in src/api.ts:42"]),
        ("build_error", "npm run build", ["npm ERR! code ELIFECYCLE", "npm ERR! errno 1", "npm ERR! web@0.1.0 build: `vite build`"]),
        ("build_error", "make", ["gcc -O2 -c main.c", "main.c:3:10: fatal error: cuda_runtime.h: No such file or directory", "make: *** [Makefile:12: main.o] Error 1"]),
        ("port_in_use", "npm run dev", ["Error: listen EADDRINUSE: address already in use :::3000"]),
        ("port_in_use", "uvicorn app:app --port 8000", ["ERROR:    [Errno 98] Address already in use"]),
        ("network", "huggingface-cli download meta-llama/Llama-3.1-8B", [f"HTTPError: 401 Client Error: Unauthorized for url: https://huggingface.co/api/models?token={token}"]),
    ]
    kind, command, texts = rng.choice(cases)
    # The final error line keeps its real numbers (errno, codes); context lines vary.
    context = [Line(text if rng.random() < 0.5 else scramble_digits(rng, text)) for text in texts[:-1]]
    lines = filler(rng) + context + [Line(texts[-1])]
    lines[-1].facts.append(error(kind))
    if token in lines[-1].text:
        lines[-1].facts.append(secret(token))
    edges = [kind]
    if rng.random() < 0.3:
        lines.insert(0, Line(rng.choice(["DeprecationWarning: pkg_resources is deprecated as an API", "WARNING: Running pip as the 'root' user can result in broken permissions"]), [warning()]))
        edges.append("warning")
    if rng.random() < 0.15:
        lines.insert(0, secret_line(rng))
        edges.append("secret")
    if kind == "network" and "port=443" in texts[-1]:
        edges.append("port_distractor")
    return example(f"error-{kind}-{index:04d}", "errors", kind, edges, command, lines)


# ---- waiting for input -----------------------------------------------------------

PROMPTS = [
    ("confirm", "apt install nvtop", "Do you want to continue? [Y/n] "),
    ("confirm", "npm install", "Need to install the following packages:\ncreate-vite@5.5.2\nOk to proceed? (y) "),
    ("confirm", "ssh pop-os", "Are you sure you want to continue connecting (yes/no/[fingerprint])? "),
    ("confirm", "python cleanup.py", "Delete 1,204 cached files? [y/N] "),
    ("password", "sudo systemctl restart nginx", "[sudo] password for sohith: "),
    ("password", "git push", "Enter passphrase for key '/home/sohith/.ssh/id_ed25519': "),
    ("password", "psql -h db -U admin", "Password for user admin: "),
    ("text", "git push", "Username for 'https://github.com': "),
    ("text", "npm create vite@latest", "? Project name: › vite-project"),
    ("text", "python setup_wizard.py", "Enter the dataset path: "),
    ("key", "./install.sh", "Press ENTER to continue, or Ctrl-C to abort"),
    ("key", "cmd.exe /c pause", "Press any key to continue . . ."),
]


def prompts(rng, index):
    kind, command, text = rng.choice(PROMPTS)
    user = rng.choice(["sohith", "sohithvishnu", "admin", "ubuntu", "dev"])
    text = text.replace("sohith", user).replace("1,204", f"{rng.randrange(2, 9999):,}").replace("dataset path", rng.choice(["dataset path", "output directory", "model name", "API base URL"]))
    *before, last = text.split("\n")
    lines = [Line("Reading package lists... Done"), Line("Building dependency tree... Done")] if command.startswith("apt") else filler(rng)
    lines += [Line(part) for part in before]
    edges = [kind]
    if rng.random() < 0.3:
        # Already answered: output continued after the prompt, so nothing is waiting.
        lines.append(Line(last + rng.choice(["y", "yes", ""])))
        lines += [Line("Unpacking nvtop (3.0.2-1) ..."), Line("Setting up nvtop (3.0.2-1) ...")]
        edges.append("answered_prompt")
    else:
        lines.append(Line(last, [prompt(kind, last.strip())]))
    return example(f"prompt-{kind}-{index:04d}", "prompts", kind, edges, command, lines)


# ---- servers -----------------------------------------------------------------------

def servers(rng, index):
    token = "".join(rng.choice("0123456789abcdef") for _ in range(48))
    cases = [
        ("uvicorn", "uvicorn app:app --host 0.0.0.0 --port 8000", ["INFO:     Started server process [4242]", "INFO:     Waiting for application startup.", "INFO:     Uvicorn running on http://0.0.0.0:8000 (Press CTRL+C to quit)"], 8000, "http://0.0.0.0:8000"),
        ("vllm", "vllm serve Qwen/Qwen3-8B --port 8001", ["INFO 10-04 11:02:13 api_server.py:212] vLLM API server version 0.6.3", "INFO:     Uvicorn running on http://0.0.0.0:8001 (Press CTRL+C to quit)"], 8001, "http://0.0.0.0:8001"),
        ("llama", "llama-server -m qwen.gguf --port 8080 -c 4096", ["llama_model_loader: loaded meta data with 30 key-value pairs", "main: server is listening on http://127.0.0.1:8080 - starting the main loop"], 8080, "http://127.0.0.1:8080"),
        ("vite", "npm run dev", ["  VITE v7.3.6  ready in 85 ms", "", "  ➜  Local:   http://localhost:5173/", "  ➜  Network: use --host to expose"], 5173, "http://localhost:5173/"),
        ("next", "npm run dev", ["   ▲ Next.js 15.0.3", "   - Local:        http://localhost:3000", " ✓ Ready in 1.2s"], 3000, "http://localhost:3000"),
        ("flask", "flask run", [" * Serving Flask app 'app'", " * Debug mode: off", " * Running on http://127.0.0.1:5000"], 5000, "http://127.0.0.1:5000"),
        ("gradio", "python demo.py", ["Running on local URL:  http://127.0.0.1:7860", "", "To create a public link, set `share=True` in `launch()`."], 7860, "http://127.0.0.1:7860"),
        ("django", "python manage.py runserver", ["System check identified no issues (0 silenced).", "Django version 5.1, using settings 'site.settings'", "Starting development server at http://127.0.0.1:8000/"], 8000, "http://127.0.0.1:8000/"),
        ("go", "go run ./cmd/api", ["2026/10/04 11:20:01 connected to postgres at db.internal:5432", "2026/10/04 11:20:01 listening on :8080"], 8080, None),
        ("jupyter", "jupyter lab --no-browser", ["[I 2026-10-04 11:21:00.000 ServerApp] Jupyter Server 2.14.2 is running at:", f"    http://127.0.0.1:8888/lab?token={token}"], 8888, "http://127.0.0.1:8888/lab"),
    ]
    family, command, texts, port, url = rng.choice(cases)
    new_port = rng.choice([port, port, rng.randrange(3000, 9999)])
    command = command.replace(str(port), str(new_port))
    texts = [text.replace(str(port), str(new_port)) for text in texts]
    url = url.replace(str(port), str(new_port)) if url else None
    port = new_port
    lines = filler(rng, 2) + [Line(text) for text in texts]
    target = next(line for line in reversed(lines) if str(port) in line.text)
    target.facts.append(server(port, url))
    edges = [family]
    if family == "jupyter":
        target.facts.append(secret(token))
        edges.append("token_in_url")
    if family == "go":
        edges.append("port_distractor")
    if rng.random() < 0.3:
        for _ in range(rng.randrange(1, 4)):
            lines.append(Line(f'INFO:     127.0.0.1:{rng.randrange(40000, 65000)} - "{rng.choice(["GET /health", "POST /v1/chat/completions", "GET /v1/models"])} HTTP/1.1" 200 OK'))
        edges.append("request_log_after")
    return example(f"server-{family}-{index:04d}", "servers", family, edges, command, lines)


# ---- tests ---------------------------------------------------------------------------

def test_runs(rng, index):
    passed, failed, skipped = rng.randrange(1, 300), rng.choice([0, 0, 1, 2, 5]), rng.choice([0, 0, 1, 3])
    family = rng.choice(["pytest", "cargo", "jest", "vitest", "unittest", "go"])
    lines = []
    if family == "pytest":
        lines.append(Line(f"collected {passed + failed + skipped} items"))
        if failed:
            lines.append(Line("FAILED tests/test_api.py::test_rejects_bad_token - AssertionError"))
        parts = ([f"{failed} failed"] if failed else []) + [f"{passed} passed"] + ([f"{skipped} skipped"] if skipped else [])
        lines.append(Line(f"=========== {', '.join(parts)} in 3.21s ===========", [tests(passed, failed, skipped)]))
        command = "pytest -q"
    elif family == "cargo":
        lines.append(Line("running 42 tests"))
        status = "FAILED" if failed else "ok"
        lines.append(Line(f"test result: {status}. {passed} passed; {failed} failed; {skipped} ignored; 0 measured; 0 filtered out; finished in 0.31s", [tests(passed, failed, skipped)]))
        command = "cargo test --workspace"
    elif family == "jest":
        parts = ([f"{failed} failed"] if failed else []) + ([f"{skipped} skipped"] if skipped else []) + [f"{passed} passed", f"{passed + failed + skipped} total"]
        lines += [Line("Test Suites: 1 failed, 11 passed, 12 total" if failed else "Test Suites: 12 passed, 12 total"), Line(f"Tests:       {', '.join(parts)}", [tests(passed, failed, skipped)]), Line("Time:        4.812 s")]
        command = "npx jest"
    elif family == "vitest":
        parts = ([f"{failed} failed"] if failed else []) + [f"{passed} passed"] + ([f"{skipped} skipped"] if skipped else [])
        lines += [Line(" Test Files  27 passed (27)"), Line(f"      Tests  {' | '.join(parts)} ({passed + failed + skipped})", [tests(passed, failed, skipped)]), Line("   Duration  4.57s")]
        command = "pnpm vitest run"
    elif family == "unittest":
        total = passed + failed + skipped
        lines.append(Line(f"Ran {total} tests in 0.412s"))
        lines.append(Line(""))
        if failed:
            detail = f"failures={failed}" + (f", skipped={skipped}" if skipped else "")
            lines.append(Line(f"FAILED ({detail})", [tests(passed, failed, skipped)]))
        else:
            lines.append(Line(f"OK (skipped={skipped})" if skipped else "OK", [tests(passed, 0, skipped)]))
        command = "python -m unittest"
    else:
        command = "go test ./..."
        if failed:
            lines += [Line("--- FAIL: TestLogin (0.01s)"), Line("    auth_test.go:42: expected 200, got 401"), Line("FAIL\texample.com/app/auth\t0.122s", [error("test_failure")])]
        else:
            lines += [Line("ok  \texample.com/app/auth\t0.122s"), Line("ok  \texample.com/app/api\t0.301s")]
    # A finished run's completion and a test-failure error are honest extras.
    ran = passed + failed + skipped
    summary_line = next((line for line in lines if any(fact[0] == "tests" for fact in line.facts)), lines[-1])
    summary_line.facts.append(optional({"name": "progress_updated", "args": {"current": ran, "total": ran, "unit": "test"}}))
    if failed and family != "go":
        summary_line.facts.append(optional({"name": "error_detected", "args": {"kind": "test_failure"}}))
    edges = [family] + (["failures"] if failed else [])
    return example(f"tests-{family}-{index:04d}", "tests", family, edges, command, lines)


# ---- downloads -----------------------------------------------------------------------

def downloads(rng, index):
    family = rng.choice(["hf", "pip", "git", "wget"])
    edges = [family]
    if family == "hf":
        percent = rng.randrange(1, 100)
        lines = [
            Line("Fetching 4 files:  25%|██▌       | 1/4 [00:12<00:36, 12.1s/it]", [progress(1, 4, "files")]),
            Line(f"model-00002-of-00004.safetensors:  {percent}%|{'█' * (percent // 10):<10}| 2.24G/4.97G [00:31<00:38, 71.2MB/s]", [progress(percent, 100, "%")]),
        ]
        command = "huggingface-cli download Qwen/Qwen3-8B"
        edges.append("nested_progress")
    elif family == "pip":
        done = round(rng.uniform(10, 790), 1)
        lines = [
            Line("Collecting torch==2.4.0"),
            Line("  Downloading torch-2.4.0-cp311-cp311-manylinux1_x86_64.whl (797.2 MB)"),
            Line(f"     ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ {done}/797.2 MB 45.1 MB/s eta 0:00:07", [progress(done, 797.2, "MB")]),
        ]
        command = "pip install torch==2.4.0"
        edges.append("version_distractor")
    elif family == "git":
        done = rng.randrange(10, 1000)
        lines = [
            Line("Cloning into 'llama.cpp'..."),
            Line(f"Receiving objects:  {done * 100 // 1000}% ({done}/1000), 12.40 MiB | 4.21 MiB/s", [progress(done, 1000, "objects")]),
        ]
        command = "git clone https://github.com/ggml-org/llama.cpp"
        edges.append("two_progress_forms")
    else:
        percent = rng.randrange(1, 100)
        lines = [
            Line("Length: 4294967296 (4.0G) [application/octet-stream]"),
            Line(f"qwen.gguf          {percent}%[=====>        ]   1.31G  52.1MB/s    eta 52s", [progress(percent, 100, "%")]),
        ]
        command = "wget https://example.com/qwen.gguf"
    return example(f"download-{family}-{index:04d}", "downloads", family, edges, command, lines)

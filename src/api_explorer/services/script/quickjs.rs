//! QuickJS-NG, through `rquickjs`, and the sandbox around it.
//!
//! # What the sandbox denies by construction
//!
//! `rquickjs-sys` compiles four C files — `libregexp.c`, `libunicode.c`,
//! `quickjs.c`, `dtoa.c`. **`quickjs-libc.c`, which is what supplies QuickJS's
//! `std` and `os` modules — file I/O, `popen`, `exec`, sockets, `getenv` — is
//! in the vendored tree and is not compiled and not linked.** No module loader
//! is configured (`rquickjs::loader` is never named), so `require` and `import`
//! resolve to nothing, and `rquickjs-core` defines no `console` of its own.
//!
//! The global object therefore contains only ECMAScript intrinsics plus what
//! [`bind`] puts there. **The sandbox is everything dodo passes in**, which is
//! what makes it auditable: it is one file, and it is this one.
//!
//! # Intrinsics are a positive allowlist
//!
//! [`Sandbox`] names what a script gets. `Context::full` is never used. Three
//! omissions are load-bearing:
//!
//! - **`Promise`** — no microtask queue, so nothing can schedule work that
//!   outlives the synchronous run and escapes the deadline. This is the
//!   omission the deadline depends on.
//! - **`Proxy`** — a reflection surface with no upside here.
//! - **`Performance`**, **`WeakRef`** — not needed.
//!
//! ## `Eval` is registered, and `report.md` §5.2 is wrong about it
//!
//! The plan called for leaving `Eval` out, on the reasoning that `Ctx::eval`
//! reaches QuickJS through the C entry point and so "the host can compile, the
//! script cannot". **It cannot work.** `JS_AddIntrinsicEval` registers no
//! global at all — it sets `ctx->eval_internal` (`quickjs.c:57120`), and
//! `JS_EvalInternal` refuses outright when that is null
//! (`quickjs.c:37427`). Without the intrinsic dodo's *own* `ctx.eval(script)`
//! fails with `TypeError: eval is not supported`, which is exactly what it did
//! when this was first built to the plan. The global `eval` function and the
//! `Function` constructor both come from `JS_AddIntrinsicBaseObjects`
//! (`quickjs.c:57591`), which `Context::custom` always applies, so they are
//! present either way — the intrinsic only decides whether they *work*.
//!
//! So it is all or nothing, and the choice is: register `Eval`, or have no
//! engine.
//!
//! What that costs is worth stating exactly, because it is less than it
//! sounds. `eval` confers **no capability a script does not already have**: it
//! compiles a string against the same global object, and that global object has
//! no filesystem, no network, no process, no module loader, no `Proxy` and no
//! `Promise`. `eval("…")` can reach `pm`, `console`, `atob`/`btoa` and the
//! intrinsics — all of which the script could reach by writing them out. The
//! real loss is **legibility**: an obfuscated script is harder to read in the
//! consent dialog, and the static unsupported-API scan `report.md` §3.4c plans
//! for Round E is defeatable by construction. That should be said in that
//! round's design rather than assumed away.
//!
//! Playing whack-a-mole instead — deleting `globalThis.eval`, replacing
//! `Function.prototype.constructor` — was considered and rejected: `Function`
//! is reachable from every function object and every generator, so it is a
//! denylist with holes, and a denylist with holes is worse than an honest
//! statement of what is allowed.
//!
//! # `pm.sendRequest` is denied by not existing
//!
//! Per `decision-pm-sendrequest-scope`: the binding is **not registered**,
//! rather than registered as a stub that throws. What makes the failure legible
//! is [`models::script::unsupported`], which turns the engine's own
//! `is not a function` into a message naming the API — so the user learns why
//! their imported script did not work instead of reading an opaque `TypeError`.
//!
//! # Bounds
//!
//! A fresh [`Runtime`] per run, with a 2 s deadline
//! ([`Runtime::set_interrupt_handler`], polled by QuickJS during execution), a
//! 16 MiB memory cap and a 256 KiB stack cap. Beyond the engine, the *outcome*
//! is bounded too — a script cannot OOM QuickJS but it can hand the host an
//! unbounded pile of console output, so [`models::script::limits`] caps that
//! and the truncation is counted rather than hidden.
//!
//! **Cancellation, honestly:** dropping the GPUI `Task` cancels the await, not
//! the blocking closure — already true of `Transport::execute`. The deadline is
//! therefore the *only* thing that bounds a runaway script. Nothing here claims
//! more.
//!
//! # `pm.test` and `pm.expect` are written in JavaScript, on purpose
//!
//! [`PRELUDE`] is the chai-shaped assertion surface, evaluated in the sandbox
//! before the user's script. Writing `.to.be.a("string")` as Rust bindings would
//! mean a chain of host objects with getters for every link (`to`, `be`, `have`,
//! `not`, …), which is both more code and a *larger* audit surface than a page
//! of JavaScript that can reach nothing the script could not reach anyway.
//!
//! It is handed its two host functions **as arguments** rather than through
//! globals, so `__record`-shaped names never exist for a script to find. What it
//! builds is ordinary script-visible data: a hostile script can overwrite
//! `pm.expect`, and that only makes its own tests lie.
//!
//! # Checking without running
//!
//! [`QuickJsEngine::check`] compiles with `Module::declare`, which is QuickJS's
//! `JS_EVAL_FLAG_COMPILE_ONLY` — the parser runs, nothing executes, no binding
//! is installed. Two notes on the fit, because it is not exact:
//!
//! - It compiles as a **module**, which is always strict. That is not a
//!   mismatch: `Ctx::eval` uses `EvalOptions::default()`, whose `strict` is
//!   already `true`, so a script runs strict here too.
//! - A module accepts a few things a script does not (top-level `await`,
//!   `import`). Those are **false negatives** — the check stays quiet and the
//!   run reports the failure — which is the safe direction. A check that
//!   underlined code the engine then ran happily would be the contradiction
//!   worth avoiding.
//!
//! [`models::script::limits`]: crate::api_explorer::models::script::limits
//! [`models::script::unsupported`]: crate::api_explorer::models::script::unsupported

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use rquickjs::function::Rest;
use rquickjs::{Context, Ctx, Function, Module, Object, Runtime, Value, context::intrinsic};

use crate::api_explorer::models::console::{ConsoleEntry, ConsoleLevel};
use crate::api_explorer::models::script::{
    DEADLINE, MEMORY_LIMIT, STACK_LIMIT, ScriptError, ScriptRequest, ScriptResponse, ScriptRun,
    ScriptSyntaxError, VariableWrite, WriteScope, limits, unsupported,
};
use crate::api_explorer::models::test_result::{ScriptPhase, TestOutcome, TestResult};
use crate::api_explorer::services::script::{ScriptContext, ScriptEngine};

/// The name compiled scripts are given, and therefore the name that appears in
/// the `at <name>:<line>:<column>` frame [`syntax_error`] reads the position
/// from. Distinctive so that parsing it cannot match a frame from user code.
const CHECK_NAME: &str = "dodo-script";

/// `pm.test`, `pm.expect` and `pm.response.to.*`, as JavaScript.
///
/// Evaluates to a function taking `(pm, record, now)`; see
/// [`install_assertions`] for why those arrive as arguments. The matcher set is
/// exactly the one `report.md` §3.2 names and no more — an unsupported matcher
/// must fail as a missing function, never quietly pass.
///
/// Chai's chain words (`to`, `be`, `have`, …) are self-references, so
/// `expect(x).to.be.above(1)` walks back to the same assertion object. `not` is
/// a **getter** returning a fresh negated assertion: a plain property would
/// recurse forever building it.
const PRELUDE: &str = r#"
(function (pm, record, now) {
  'use strict';

  function typeOf(value) {
    if (value === null) return 'null';
    if (Array.isArray(value)) return 'array';
    return typeof value;
  }

  function show(value) {
    if (typeof value === 'string') return JSON.stringify(value);
    if (value === undefined) return 'undefined';
    if (value === null) return 'null';
    try {
      var text = JSON.stringify(value);
      return text === undefined ? String(value) : text;
    } catch (error) {
      return String(value);
    }
  }

  function fail(message) {
    var error = new Error(message);
    error.name = 'AssertionError';
    throw error;
  }

  function deepEqual(a, b) {
    if (a === b) return true;
    if (typeOf(a) !== typeOf(b)) return false;
    if (a === null || typeof a !== 'object') return a !== a && b !== b;
    if (Array.isArray(a)) {
      if (a.length !== b.length) return false;
      for (var i = 0; i < a.length; i++) if (!deepEqual(a[i], b[i])) return false;
      return true;
    }
    var keys = Object.keys(a);
    if (keys.length !== Object.keys(b).length) return false;
    for (var k = 0; k < keys.length; k++) {
      if (!Object.prototype.hasOwnProperty.call(b, keys[k])) return false;
      if (!deepEqual(a[keys[k]], b[keys[k]])) return false;
    }
    return true;
  }

  function assertion(value, negated) {
    var self = {};

    function check(ok, message, negatedMessage) {
      if (negated) {
        if (ok) fail(negatedMessage);
      } else if (!ok) {
        fail(message);
      }
      return self;
    }

    var chain = ['to', 'be', 'been', 'is', 'that', 'and', 'has', 'have',
                 'with', 'at', 'of', 'same', 'which', 'does'];
    for (var i = 0; i < chain.length; i++) self[chain[i]] = self;

    Object.defineProperty(self, 'not', {
      get: function () { return assertion(value, !negated); }
    });

    function getter(name, run) {
      Object.defineProperty(self, name, { get: run });
    }

    self.equal = function (other) {
      return check(value === other,
        'expected ' + show(value) + ' to equal ' + show(other),
        'expected ' + show(value) + ' not to equal ' + show(other));
    };
    self.equals = self.equal;
    self.eq = self.equal;

    self.eql = function (other) {
      return check(deepEqual(value, other),
        'expected ' + show(value) + ' to deeply equal ' + show(other),
        'expected ' + show(value) + ' not to deeply equal ' + show(other));
    };

    self.a = function (type) {
      var wanted = String(type).toLowerCase();
      return check(typeOf(value) === wanted,
        'expected ' + show(value) + ' to be a ' + wanted + ' but it is a ' + typeOf(value),
        'expected ' + show(value) + ' not to be a ' + wanted);
    };
    self.an = self.a;

    getter('true', function () {
      return check(value === true,
        'expected ' + show(value) + ' to be true',
        'expected ' + show(value) + ' not to be true');
    });
    getter('false', function () {
      return check(value === false,
        'expected ' + show(value) + ' to be false',
        'expected ' + show(value) + ' not to be false');
    });
    getter('null', function () {
      return check(value === null,
        'expected ' + show(value) + ' to be null',
        'expected ' + show(value) + ' not to be null');
    });
    getter('undefined', function () {
      return check(value === undefined,
        'expected ' + show(value) + ' to be undefined',
        'expected ' + show(value) + ' not to be undefined');
    });
    getter('ok', function () {
      return check(!!value,
        'expected ' + show(value) + ' to be truthy',
        'expected ' + show(value) + ' not to be truthy');
    });

    self.include = function (needle) {
      var found = false;
      if (typeof value === 'string') {
        found = value.indexOf(String(needle)) !== -1;
      } else if (Array.isArray(value)) {
        for (var i = 0; i < value.length; i++) {
          if (deepEqual(value[i], needle)) { found = true; break; }
        }
      } else if (value !== null && typeof value === 'object' && needle !== null &&
                 typeof needle === 'object') {
        var keys = Object.keys(needle);
        found = true;
        for (var k = 0; k < keys.length; k++) {
          if (!deepEqual(value[keys[k]], needle[keys[k]])) { found = false; break; }
        }
      }
      return check(found,
        'expected ' + show(value) + ' to include ' + show(needle),
        'expected ' + show(value) + ' not to include ' + show(needle));
    };
    self.includes = self.include;
    self.contain = self.include;
    self.contains = self.include;

    self.match = function (pattern) {
      return check(pattern.test(String(value)),
        'expected ' + show(value) + ' to match ' + String(pattern),
        'expected ' + show(value) + ' not to match ' + String(pattern));
    };
    self.matches = self.match;

    self.property = function (name, expected) {
      var owner = value === null || value === undefined ? null : Object(value);
      var present = owner !== null && name in owner;
      if (arguments.length < 2) {
        return check(present,
          'expected ' + show(value) + " to have property '" + name + "'",
          'expected ' + show(value) + " not to have property '" + name + "'");
      }
      return check(present && owner[name] === expected,
        'expected ' + show(value) + " to have property '" + name + "' equal to " + show(expected),
        'expected ' + show(value) + " not to have property '" + name + "' equal to " + show(expected));
    };

    self.lengthOf = function (expected) {
      var length = value === null || value === undefined ? undefined : value.length;
      return check(length === expected,
        'expected ' + show(value) + ' to have length ' + expected + ' but it has ' + length,
        'expected ' + show(value) + ' not to have length ' + expected);
    };
    self.length = self.lengthOf;

    self.above = function (bound) {
      return check(value > bound,
        'expected ' + show(value) + ' to be above ' + bound,
        'expected ' + show(value) + ' not to be above ' + bound);
    };
    self.greaterThan = self.above;
    self.gt = self.above;

    self.below = function (bound) {
      return check(value < bound,
        'expected ' + show(value) + ' to be below ' + bound,
        'expected ' + show(value) + ' not to be below ' + bound);
    };
    self.lessThan = self.below;
    self.lt = self.below;

    self.within = function (low, high) {
      return check(value >= low && value <= high,
        'expected ' + show(value) + ' to be within ' + low + ' and ' + high,
        'expected ' + show(value) + ' not to be within ' + low + ' and ' + high);
    };

    // `.to.deep.equal(x)` is chai's spelling of `.eql(x)`.
    self.deep = { equal: self.eql, eql: self.eql, include: self.include };

    return self;
  }

  pm.expect = function (value) { return assertion(value, false); };

  pm.test = function (name, body) {
    var label = String(name);
    var started = now();
    var outcome = 'passed';
    var message = '';
    try {
      if (typeof body !== 'function') {
        throw new TypeError('pm.test needs a function as its second argument');
      }
      body();
    } catch (error) {
      if (error && error.name === 'AssertionError') {
        outcome = 'failed';
        message = String(error.message);
      } else {
        outcome = 'errored';
        message = error && error.name
          ? String(error.name) + ': ' + String(error.message)
          : String(error);
      }
    }
    record(label, outcome, message, now() - started);
  };

  if (!pm.response) return;
  var response = pm.response;

  function pathValue(root, path) {
    var parts = String(path).replace(/\[(\d+)\]/g, '.$1').split('.');
    var cursor = root;
    for (var i = 0; i < parts.length; i++) {
      if (parts[i] === '') continue;
      if (cursor === null || cursor === undefined) return undefined;
      cursor = cursor[parts[i]];
    }
    return cursor;
  }

  var have = {
    status: function (expected) {
      if (typeof expected === 'number') {
        if (response.code !== expected) {
          fail('expected response code ' + response.code + ' to equal ' + expected);
        }
      } else if (String(response.status) !== String(expected)) {
        fail('expected response status ' + show(response.status) +
             ' to equal ' + show(expected));
      }
      return response.to;
    },
    header: function (name, expected) {
      if (!response.headers.has(name)) {
        fail("expected the response to have header '" + name + "'");
      }
      if (arguments.length > 1 &&
          String(response.headers.get(name)) !== String(expected)) {
        fail("expected header '" + name + "' to be " + show(expected) +
             ' but it is ' + show(response.headers.get(name)));
      }
      return response.to;
    },
    body: function (expected) {
      var text = response.text();
      if (expected instanceof RegExp) {
        if (!expected.test(text)) {
          fail('expected the response body to match ' + String(expected));
        }
      } else if (arguments.length > 0 && text !== String(expected)) {
        fail('expected the response body to equal ' + show(expected));
      } else if (arguments.length === 0 && text.length === 0) {
        fail('expected the response to have a body');
      }
      return response.to;
    },
    jsonBody: function (path, expected) {
      var parsed;
      try {
        parsed = response.json();
      } catch (error) {
        fail('expected the response body to be JSON');
      }
      if (arguments.length === 0) return response.to;
      var found = pathValue(parsed, path);
      if (arguments.length === 1) {
        if (found === undefined) {
          fail("expected the response body to have '" + path + "'");
        }
        return response.to;
      }
      if (!deepEqual(found, expected)) {
        fail("expected '" + path + "' to equal " + show(expected) +
             ' but it is ' + show(found));
      }
      return response.to;
    }
  };

  var be = {};
  Object.defineProperty(be, 'json', {
    get: function () {
      try {
        response.json();
      } catch (error) {
        fail('expected the response body to be JSON');
      }
      return response.to;
    }
  });

  response.to = { have: have, has: have, be: be, is: be };
})
"#;

/// The intrinsics a script gets. Everything absent from this tuple is absent
/// from the language — see this module's doc for the four that matter.
type Sandbox = (
    // Required for `Ctx::eval` to run anything at all — see this module's doc.
    intrinsic::Eval,
    intrinsic::Date,
    intrinsic::RegExp,
    intrinsic::RegExpCompiler,
    intrinsic::Json,
    intrinsic::MapSet,
    intrinsic::TypedArrays,
);

/// The engine dodo ships.
pub struct QuickJsEngine;

impl ScriptEngine for QuickJsEngine {
    fn run(&self, script: &str, context: ScriptContext) -> ScriptRun {
        run(script, context)
    }

    fn check(&self, script: &str) -> Option<ScriptSyntaxError> {
        check(script)
    }
}

/// Everything one run accumulates, shared with every binding.
///
/// `Rc<RefCell<_>>` rather than a lock: the runtime, the context, the bindings
/// and this state are all created, used and dropped inside one call on one
/// thread. Nothing here is `Send`, and nothing needs to be.
struct RunState {
    /// Headers as the script has them now. Method, URL and body live as
    /// ordinary JavaScript properties on `pm.request` and are read back at the
    /// end; headers need methods (`add`/`upsert`/`remove`), so they live here.
    headers: Vec<(String, String)>,
    variables: crate::api_explorer::models::variables::VariableSet,
    /// `pm.variables.set` — this run only.
    locals: Vec<(String, String)>,
    environment: BTreeMap<String, String>,
    collection: BTreeMap<String, String>,
    writes: Vec<VariableWrite>,
    logs: Vec<ConsoleEntry>,
    log_bytes: usize,
    dropped_logs: usize,
    /// What `pm.test` has produced so far, in call order.
    tests: Vec<TestResult>,
    dropped_tests: usize,
}

impl RunState {
    fn log(&mut self, level: ConsoleLevel, message: String) {
        if self.logs.len() >= limits::CONSOLE_ENTRIES
            || self.log_bytes.saturating_add(message.len()) > limits::CONSOLE_BYTES
        {
            self.dropped_logs += 1;
            return;
        }
        self.log_bytes += message.len();
        self.logs.push(ConsoleEntry::script(level, message));
    }

    /// The merged view `pm.variables.get` reads: this run's own values first,
    /// then the scopes a script may have edited, then everything configured.
    fn lookup(&self, name: &str) -> Option<String> {
        let name = name.trim();
        if let Some((_, value)) = self.locals.iter().find(|(key, _)| key == name) {
            return Some(value.clone());
        }
        self.environment
            .get(name)
            .or_else(|| self.collection.get(name))
            .cloned()
            .or_else(|| {
                self.variables
                    .lookup(name)
                    .map(|(_, value)| value.to_string())
            })
    }

    /// Records a write bound for a persisted scope. Over-budget writes are
    /// dropped rather than allowed to grow without limit; the caller sees the
    /// count.
    fn write(&mut self, scope: WriteScope, key: String, value: Option<String>) {
        if self.writes.len() >= limits::VARIABLE_WRITES {
            return;
        }
        let value = value.map(|value| truncate(value, limits::VARIABLE_VALUE_BYTES));
        let map = match scope {
            WriteScope::Environment => &mut self.environment,
            WriteScope::Collection => &mut self.collection,
        };
        match &value {
            Some(value) => {
                map.insert(key.clone(), value.clone());
            }
            None => {
                map.remove(&key);
            }
        }
        self.writes.push(VariableWrite { scope, key, value });
    }

    /// Records one finished `pm.test`. Over-budget results are dropped and
    /// counted, the same bargain the console makes.
    fn record_test(&mut self, result: TestResult) {
        if self.tests.len() >= limits::TEST_RESULTS {
            self.dropped_tests += 1;
            return;
        }
        self.tests.push(result);
    }
}

/// Keeps a value inside its cap without splitting a character.
fn truncate(mut value: String, cap: usize) -> String {
    if value.len() <= cap {
        return value;
    }
    let mut end = cap;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value
}

/// One run, start to finish.
fn run(script: &str, context: ScriptContext) -> ScriptRun {
    let started = Instant::now();
    let ScriptContext {
        phase,
        request: original,
        response,
        variables,
        environment,
        collection,
        request_name,
    } = context;

    let state = Rc::new(RefCell::new(RunState {
        headers: original.headers.clone(),
        variables,
        locals: Vec::new(),
        environment,
        collection,
        writes: Vec::new(),
        logs: Vec::new(),
        log_bytes: 0,
        dropped_logs: 0,
        tests: Vec::new(),
        dropped_tests: 0,
    }));

    let Ok(runtime) = Runtime::new() else {
        return ScriptRun::failed(ScriptError::OutOfMemory);
    };
    runtime.set_memory_limit(MEMORY_LIMIT);
    runtime.set_max_stack_size(STACK_LIMIT);

    // QuickJS polls this during execution — the only thing that stops an
    // unbounded loop. `Instant` is monotonic, so a clock change cannot extend
    // or collapse the budget.
    let deadline = started + DEADLINE;
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));

    let Ok(context_handle) = Context::custom::<Sandbox>(&runtime) else {
        return ScriptRun::failed(ScriptError::OutOfMemory);
    };

    let evaluated = context_handle.with(|ctx| {
        if let Err(error) = bind(
            &ctx,
            &state,
            &original,
            response.as_ref(),
            phase,
            &request_name,
            started,
        ) {
            return Err(describe(&ctx, error, script));
        }
        match ctx.eval::<Value, _>(script) {
            Ok(_) => Ok(read_back(&ctx)),
            Err(error) => Err(describe(&ctx, error, script)),
        }
    });

    let mut run = ScriptRun {
        duration: started.elapsed(),
        ..ScriptRun::default()
    };

    {
        let state = state.borrow();
        run.logs = state.logs.clone();
        run.writes = state.writes.clone();
        run.locals = state.locals.clone();
        run.environment = state.environment.clone();
        run.collection = state.collection.clone();
        run.dropped_logs = state.dropped_logs;
        // Tests a run defined before it threw are kept for the same reason its
        // logs are: they are usually the only account of how far it got.
        run.tests = state.tests.clone();
        run.dropped_tests = state.dropped_tests;
    }

    match evaluated {
        // The request write-back is a pre-request concern only. A post-response
        // script that assigns to `pm.request` has changed a request that has
        // already gone; reading it back would make the send path apply it to
        // nothing.
        Ok(fields) if phase == ScriptPhase::PreRequest => {
            let request = ScriptRequest {
                method: fields.method,
                url: fields.url,
                headers: state.borrow().headers.clone(),
                body: fields.body,
            };
            // `None` means "left alone", which is what lets the send path skip
            // the write-back entirely and keep disabled header rows intact.
            run.request = (request != original).then_some(request);
        }
        Ok(_) => {}
        Err(error) => {
            // The interrupt handler reports the deadline as an ordinary
            // exception, so the clock is what tells the two apart.
            run.error = Some(if Instant::now() >= deadline {
                ScriptError::Deadline {
                    seconds: DEADLINE.as_secs(),
                }
            } else {
                error
            });
        }
    }

    run
}

/// Compiles `script` and reports where it does not parse. Nothing runs; see
/// this module's doc for what a module compile does and does not answer.
fn check(script: &str) -> Option<ScriptSyntaxError> {
    if script.trim().is_empty() {
        return None;
    }

    let runtime = Runtime::new().ok()?;
    runtime.set_memory_limit(MEMORY_LIMIT);
    runtime.set_max_stack_size(STACK_LIMIT);
    // A pathological source can make a parser work; the same budget the run
    // gets bounds the check, so typing can never wedge the executor.
    let deadline = Instant::now() + DEADLINE;
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));

    let context = Context::custom::<Sandbox>(&runtime).ok()?;
    context.with(
        |ctx| match Module::declare(ctx.clone(), CHECK_NAME, script) {
            Ok(_) => None,
            Err(_) => Some(syntax_error(&ctx)),
        },
    )
}

/// Reads a compile failure out of the pending exception.
///
/// The position comes from the first stack frame, which QuickJS writes as
/// `    at <filename>:<line>:<column>` with both numbers 1-based
/// (`quickjs.c`, `build_backtrace`). A frame that cannot be read leaves the
/// caret at the start of the script rather than nowhere: an underline in the
/// wrong place is still a message, and no underline at all is silence.
fn syntax_error(ctx: &Ctx<'_>) -> ScriptSyntaxError {
    let caught = ctx.catch();
    let detail = caught
        .clone()
        .into_object()
        .and_then(|object| {
            let exception = rquickjs::Exception::from_object(object)?;
            exception.message()
        })
        .unwrap_or_else(|| "SyntaxError".to_string());

    let stack = caught
        .into_object()
        .and_then(|object| object.get::<_, Option<String>>("stack").ok().flatten())
        .unwrap_or_default();

    let (line, column) = position(&stack).unwrap_or((1, 1));
    ScriptSyntaxError {
        // 1-based from the engine, 0-based for the editor.
        line: line.saturating_sub(1),
        column: column.saturating_sub(1),
        detail,
    }
}

/// The `line, column` of the `at <CHECK_NAME>:line:column` frame.
fn position(stack: &str) -> Option<(usize, usize)> {
    let after = stack.split_once(&format!("{CHECK_NAME}:"))?.1;
    let mut numbers = after
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty());
    let line = numbers.next()?.parse().ok()?;
    let column = numbers
        .next()
        .and_then(|part| part.parse().ok())
        .unwrap_or(1);
    Some((line, column))
}

/// The `pm.request` fields that live as plain JavaScript properties.
struct Fields {
    method: String,
    url: String,
    body: String,
}

/// Reads `pm.request`'s data properties back after the script has run.
///
/// A script that deleted or replaced them with something unreadable gets the
/// empty string, which `prepare` then rejects with its own clear message —
/// better than this layer inventing one.
fn read_back(ctx: &Ctx<'_>) -> Fields {
    let request: Option<Object> = ctx
        .globals()
        .get::<_, Object>("pm")
        .ok()
        .and_then(|pm| pm.get::<_, Object>("request").ok());

    let field = |name: &str| {
        request
            .as_ref()
            .and_then(|request| {
                request
                    .get::<_, Option<rquickjs::Coerced<String>>>(name)
                    .ok()
            })
            .flatten()
            .map(|value| value.0)
            .unwrap_or_default()
    };

    Fields {
        method: field("method"),
        url: field("url"),
        body: field("body"),
    }
}

/// Turns an engine error into one dodo can show.
///
/// `source` is the script, used only as the fallback in
/// [`unsupported`] — QuickJS does not name the callee in
/// `TypeError: not a function`.
fn describe(ctx: &Ctx<'_>, error: rquickjs::Error, source: &str) -> ScriptError {
    if matches!(error, rquickjs::Error::Allocation) {
        return ScriptError::OutOfMemory;
    }

    let caught = ctx.catch();
    // QuickJS raises `InternalError: out of memory`, but building that error
    // object itself allocates — so a *re-entrant* failure leaves the exception
    // unset (`JS_ThrowOutOfMemory`, `quickjs.c:8127`) and `catch()` hands back
    // null. Reporting that as `Threw { detail: "null" }` would be the least
    // useful message available.
    if caught.is_null() || caught.is_undefined() {
        return ScriptError::OutOfMemory;
    }
    let detail = caught
        .clone()
        .into_object()
        .and_then(|object| {
            let exception = rquickjs::Exception::from_object(object)?;
            let message = exception.message()?;
            Some(
                match exception.get::<_, Option<rquickjs::Coerced<String>>>("name") {
                    Ok(Some(name)) => format!("{}: {message}", name.0),
                    _ => message,
                },
            )
        })
        .or_else(|| {
            caught
                .get::<rquickjs::Coerced<String>>()
                .ok()
                .map(|value| value.0)
        })
        .unwrap_or_else(|| error.to_string());

    if detail.contains("out of memory") || detail.contains("stack overflow") {
        return ScriptError::OutOfMemory;
    }
    if let Some(name) = unsupported(&detail, source) {
        return ScriptError::Unsupported { name: name.into() };
    }
    ScriptError::Threw { detail }
}

/// Installs every binding a script gets. Read this function as the sandbox's
/// allowlist: what is not here does not exist.
#[allow(clippy::too_many_arguments)]
fn bind<'js>(
    ctx: &Ctx<'js>,
    state: &Rc<RefCell<RunState>>,
    request: &ScriptRequest,
    response: Option<&ScriptResponse>,
    phase: ScriptPhase,
    request_name: &str,
    started: Instant,
) -> Result<(), rquickjs::Error> {
    let globals = ctx.globals();

    globals.set("console", console(ctx, state)?)?;
    globals.set("atob", Function::new(ctx.clone(), atob)?)?;
    globals.set("btoa", Function::new(ctx.clone(), btoa)?)?;

    let pm = Object::new(ctx.clone())?;
    pm.set("variables", variables_binding(ctx, state)?)?;
    pm.set(
        "environment",
        scope_binding(ctx, state, WriteScope::Environment)?,
    )?;
    pm.set(
        "collectionVariables",
        scope_binding(ctx, state, WriteScope::Collection)?,
    )?;
    pm.set("request", request_binding(ctx, state, request)?)?;

    // Absent in the pre-request phase rather than present and empty: reaching
    // for a response that does not exist yet must fail loudly, not read as a
    // status of 0.
    if let Some(response) = response {
        pm.set("response", response_binding(ctx, response)?)?;
    }

    let info = Object::new(ctx.clone())?;
    info.set("requestName", request_name)?;
    info.set("eventName", phase.event_name())?;
    pm.set("info", info)?;

    globals.set("pm", pm.clone())?;

    install_assertions(ctx, state, &pm, phase, started)?;
    Ok(())
}

/// Evaluates [`PRELUDE`] and hands it its two host functions as arguments.
///
/// They are arguments rather than globals so that no `__`-prefixed name exists
/// for a script to find and call directly; after this returns, the only way to
/// record a result is `pm.test`.
fn install_assertions<'js>(
    ctx: &Ctx<'js>,
    state: &Rc<RefCell<RunState>>,
    pm: &Object<'js>,
    phase: ScriptPhase,
    started: Instant,
) -> Result<(), rquickjs::Error> {
    let recorder = state.clone();
    let record = Function::new(
        ctx.clone(),
        move |name: String, outcome: String, message: String, micros: f64| {
            let outcome = match outcome.as_str() {
                "passed" => TestOutcome::Passed,
                "failed" => TestOutcome::Failed { message },
                _ => TestOutcome::Errored { message },
            };
            recorder.borrow_mut().record_test(TestResult {
                name,
                outcome,
                elapsed: Duration::from_micros(micros.max(0.) as u64),
                phase,
            });
        },
    )?;

    // Microseconds since the run began. `Date.now` would do, but at millisecond
    // resolution every assertion reads as 0 ms, which looks like a bug.
    let now = Function::new(ctx.clone(), move || started.elapsed().as_micros() as f64)?;

    let installer: Function = ctx.eval(PRELUDE)?;
    installer.call::<_, ()>((pm.clone(), record, now))
}

/// `pm.response` — everything about what came back, and nothing that could
/// change it.
fn response_binding<'js>(
    ctx: &Ctx<'js>,
    response: &ScriptResponse,
) -> Result<Object<'js>, rquickjs::Error> {
    let object = Object::new(ctx.clone())?;
    object.set("code", response.code)?;
    object.set("status", response.status.as_str())?;
    object.set("responseTime", response.elapsed_millis)?;
    object.set("responseSize", response.size_bytes)?;

    let text = response.body.clone();
    object.set(
        "text",
        Function::new(ctx.clone(), move || text.clone())?.with_name("text")?,
    )?;

    let json_source = response.body.clone();
    object.set(
        "json",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>| {
            // Postman throws a JSONError for a body that is not JSON, and a
            // script that catches it is doing something reasonable. Returning
            // `undefined` instead would turn a wrong content type into a silent
            // `undefined is not an object` three lines later.
            ctx.json_parse(json_source.clone()).map_err(|_| {
                rquickjs::Exception::throw_message(
                    &ctx,
                    "JSONError: the response body is not valid JSON",
                )
            })
        })?
        .with_name("json")?,
    )?;

    let headers = Object::new(ctx.clone())?;

    let get = response.headers.clone();
    headers.set(
        "get",
        Function::new(ctx.clone(), move |name: String| {
            let name = name.trim().to_ascii_lowercase();
            get.iter()
                .find(|(key, _)| key.trim().to_ascii_lowercase() == name)
                .map(|(_, value)| value.clone())
        })?,
    )?;

    let has = response.headers.clone();
    headers.set(
        "has",
        Function::new(ctx.clone(), move |name: String| {
            let name = name.trim().to_ascii_lowercase();
            has.iter()
                .any(|(key, _)| key.trim().to_ascii_lowercase() == name)
        })?,
    )?;

    let all = response.headers.clone();
    headers.set(
        "all",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>| {
            let list = rquickjs::Array::new(ctx.clone())?;
            for (index, (key, value)) in all.iter().enumerate() {
                let entry = Object::new(ctx.clone())?;
                entry.set("key", key.as_str())?;
                entry.set("value", value.as_str())?;
                list.set(index, entry)?;
            }
            Ok::<_, rquickjs::Error>(list)
        })?,
    )?;

    object.set("headers", headers)?;
    Ok(object)
}

/// `console.log/info/warn/error/debug`.
fn console<'js>(
    ctx: &Ctx<'js>,
    state: &Rc<RefCell<RunState>>,
) -> Result<Object<'js>, rquickjs::Error> {
    let console = Object::new(ctx.clone())?;
    for (name, level) in [
        ("debug", ConsoleLevel::Debug),
        ("log", ConsoleLevel::Log),
        ("info", ConsoleLevel::Log),
        ("warn", ConsoleLevel::Warn),
        ("error", ConsoleLevel::Error),
    ] {
        let state = state.clone();
        console.set(
            name,
            Function::new(ctx.clone(), move |ctx: Ctx<'js>, args: Rest<Value<'js>>| {
                let text = args
                    .0
                    .iter()
                    .map(|value| display(&ctx, value))
                    .collect::<Vec<_>>()
                    .join(" ");
                state.borrow_mut().log(level, text);
            })?,
        )?;
    }
    Ok(console)
}

/// How a logged value reads. Strings go through as themselves — quoting them
/// would make the common `console.log(someString)` noisy — and everything else
/// is JSON, falling back to coercion for what JSON cannot express.
fn display<'js>(ctx: &Ctx<'js>, value: &Value<'js>) -> String {
    if let Some(string) = value.as_string() {
        return string.to_string().unwrap_or_default();
    }
    if value.is_undefined() {
        return "undefined".into();
    }
    if value.is_null() {
        return "null".into();
    }
    ctx.json_stringify(value.clone())
        .ok()
        .flatten()
        .and_then(|json| json.to_string().ok())
        .or_else(|| {
            value
                .clone()
                .get::<rquickjs::Coerced<String>>()
                .ok()
                .map(|coerced| coerced.0)
        })
        .unwrap_or_default()
}

/// `pm.variables` — the merged read view, plus run-local writes.
fn variables_binding<'js>(
    ctx: &Ctx<'js>,
    state: &Rc<RefCell<RunState>>,
) -> Result<Object<'js>, rquickjs::Error> {
    let object = Object::new(ctx.clone())?;

    let get = state.clone();
    object.set(
        "get",
        Function::new(ctx.clone(), move |name: String| get.borrow().lookup(&name))?,
    )?;

    let has = state.clone();
    object.set(
        "has",
        Function::new(ctx.clone(), move |name: String| {
            has.borrow().lookup(&name).is_some()
        })?,
    )?;

    let set = state.clone();
    object.set(
        "set",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, name: String, value: Value<'js>| {
                let value = truncate(display(&ctx, &value), limits::VARIABLE_VALUE_BYTES);
                let mut state = set.borrow_mut();
                let name = name.trim().to_string();
                let room = state.locals.len() < limits::VARIABLE_WRITES;
                match state.locals.iter_mut().find(|(key, _)| *key == name) {
                    Some(slot) => slot.1 = value,
                    None if room => state.locals.push((name, value)),
                    None => {}
                }
            },
        )?,
    )?;

    let unset = state.clone();
    object.set(
        "unset",
        Function::new(ctx.clone(), move |name: String| {
            let name = name.trim().to_string();
            unset.borrow_mut().locals.retain(|(key, _)| *key != name);
        })?,
    )?;

    Ok(object)
}

/// The map one scope's methods read.
fn read(state: &RunState, scope: WriteScope) -> &BTreeMap<String, String> {
    match scope {
        WriteScope::Environment => &state.environment,
        WriteScope::Collection => &state.collection,
    }
}

/// `pm.environment` and `pm.collectionVariables` — the same five methods over
/// two different persisted scopes.
fn scope_binding<'js>(
    ctx: &Ctx<'js>,
    state: &Rc<RefCell<RunState>>,
    scope: WriteScope,
) -> Result<Object<'js>, rquickjs::Error> {
    let object = Object::new(ctx.clone())?;

    let get = state.clone();
    object.set(
        "get",
        Function::new(ctx.clone(), move |name: String| {
            read(&get.borrow(), scope).get(name.trim()).cloned()
        })?,
    )?;

    let has = state.clone();
    object.set(
        "has",
        Function::new(ctx.clone(), move |name: String| {
            read(&has.borrow(), scope).contains_key(name.trim())
        })?,
    )?;

    let set = state.clone();
    object.set(
        "set",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, name: String, value: Value<'js>| {
                let value = display(&ctx, &value);
                set.borrow_mut()
                    .write(scope, name.trim().to_string(), Some(value));
            },
        )?,
    )?;

    let unset = state.clone();
    object.set(
        "unset",
        Function::new(ctx.clone(), move |name: String| {
            unset
                .borrow_mut()
                .write(scope, name.trim().to_string(), None);
        })?,
    )?;

    let to_object = state.clone();
    object.set(
        "toObject",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>| {
            let result = Object::new(ctx)?;
            for (key, value) in read(&to_object.borrow(), scope) {
                result.set(key.as_str(), value.as_str())?;
            }
            Ok::<_, rquickjs::Error>(result)
        })?,
    )?;

    Ok(object)
}

/// `pm.request` — plain properties for method/URL/body, and a headers object
/// with Postman's five methods.
fn request_binding<'js>(
    ctx: &Ctx<'js>,
    state: &Rc<RefCell<RunState>>,
    request: &ScriptRequest,
) -> Result<Object<'js>, rquickjs::Error> {
    let object = Object::new(ctx.clone())?;
    object.set("method", request.method.as_str())?;
    object.set("url", request.url.as_str())?;
    object.set("body", request.body.as_str())?;

    let headers = Object::new(ctx.clone())?;

    let get = state.clone();
    headers.set(
        "get",
        Function::new(ctx.clone(), move |name: String| {
            let name = name.trim().to_ascii_lowercase();
            get.borrow()
                .headers
                .iter()
                .find(|(key, _)| key.trim().to_ascii_lowercase() == name)
                .map(|(_, value)| value.clone())
        })?,
    )?;

    let has = state.clone();
    headers.set(
        "has",
        Function::new(ctx.clone(), move |name: String| {
            let name = name.trim().to_ascii_lowercase();
            has.borrow()
                .headers
                .iter()
                .any(|(key, _)| key.trim().to_ascii_lowercase() == name)
        })?,
    )?;

    let add = state.clone();
    headers.set(
        "add",
        Function::new(ctx.clone(), move |header: Value<'js>| {
            if let Some((key, value)) = header_pair(&header) {
                add.borrow_mut().headers.push((key, value));
            }
        })?,
    )?;

    let upsert = state.clone();
    headers.set(
        "upsert",
        Function::new(ctx.clone(), move |header: Value<'js>| {
            let Some((key, value)) = header_pair(&header) else {
                return;
            };
            let mut state = upsert.borrow_mut();
            let needle = key.trim().to_ascii_lowercase();
            match state
                .headers
                .iter_mut()
                .find(|(existing, _)| existing.trim().to_ascii_lowercase() == needle)
            {
                Some(slot) => slot.1 = value,
                None => state.headers.push((key, value)),
            }
        })?,
    )?;

    let remove = state.clone();
    headers.set(
        "remove",
        Function::new(ctx.clone(), move |name: String| {
            let needle = name.trim().to_ascii_lowercase();
            remove
                .borrow_mut()
                .headers
                .retain(|(key, _)| key.trim().to_ascii_lowercase() != needle);
        })?,
    )?;

    let all = state.clone();
    headers.set(
        "all",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>| {
            let list = rquickjs::Array::new(ctx.clone())?;
            for (index, (key, value)) in all.borrow().headers.iter().enumerate() {
                let entry = Object::new(ctx.clone())?;
                entry.set("key", key.as_str())?;
                entry.set("value", value.as_str())?;
                list.set(index, entry)?;
            }
            Ok::<_, rquickjs::Error>(list)
        })?,
    )?;

    object.set("headers", headers)?;
    Ok(object)
}

/// A `{ key, value }` object as the header methods accept it. Postman also
/// accepts a `"Name: value"` string, and so does this.
fn header_pair(header: &Value<'_>) -> Option<(String, String)> {
    if let Some(string) = header.as_string() {
        let text = string.to_string().ok()?;
        let (key, value) = text.split_once(':')?;
        return Some((key.trim().to_string(), value.trim().to_string()));
    }
    let object = header.as_object()?;
    let key: String = object.get("key").ok()?;
    let value: String = object
        .get::<_, Option<rquickjs::Coerced<String>>>("value")
        .ok()
        .flatten()
        .map(|value| value.0)
        .unwrap_or_default();
    Some((key, value))
}

/// `atob` — base64 to a binary string, the browser's semantics.
fn atob(ctx: Ctx<'_>, encoded: String) -> rquickjs::Result<String> {
    let compact: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();
    match STANDARD.decode(compact.as_bytes()) {
        // Each byte becomes one code point, which is what a browser's `atob`
        // returns: a "binary string", not UTF-8 text.
        Ok(bytes) => Ok(bytes.into_iter().map(char::from).collect()),
        Err(_) => Err(rquickjs::Exception::throw_message(
            &ctx,
            "atob: the string is not valid base64",
        )),
    }
}

/// `btoa` — a binary string to base64. A code point above 255 is an error, as
/// it is in a browser, rather than a silently mangled encoding.
fn btoa(ctx: Ctx<'_>, raw: String) -> rquickjs::Result<String> {
    let mut bytes = Vec::with_capacity(raw.len());
    for character in raw.chars() {
        let code = character as u32;
        if code > 0xff {
            return Err(rquickjs::Exception::throw_message(
                &ctx,
                "btoa: the string contains a character outside the Latin-1 range",
            ));
        }
        bytes.push(code as u8);
    }
    Ok(STANDARD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::{QuickJsEngine, check, position, run};
    use crate::api_explorer::models::console::ConsoleLevel;
    use crate::api_explorer::models::script::{
        ScriptError, ScriptRequest, ScriptResponse, ScriptRun, WriteScope, limits,
    };
    use crate::api_explorer::models::test_result::{ScriptPhase, TestOutcome};
    use crate::api_explorer::models::variables::{Variable, VariableScope, VariableSet};
    use crate::api_explorer::services::script::{ScriptContext, ScriptEngine};
    use std::collections::BTreeMap;

    fn context() -> ScriptContext {
        let mut variables = VariableSet::default();
        variables.push_layer(
            VariableScope::Collection,
            vec![Variable::new("version", "v1")],
        );
        variables.push_layer(
            VariableScope::Environment,
            vec![Variable::new("host", "example.com")],
        );

        ScriptContext {
            phase: ScriptPhase::PreRequest,
            request: ScriptRequest {
                method: "GET".into(),
                url: "https://example.com/things".into(),
                headers: vec![("Accept".into(), "application/json".into())],
                body: String::new(),
            },
            response: None,
            variables,
            environment: BTreeMap::from([("host".into(), "example.com".into())]),
            collection: BTreeMap::from([("version".into(), "v1".into())]),
            request_name: "List things".into(),
        }
    }

    /// The same context, in the post-response phase, with a response to read.
    fn responded(body: &str) -> ScriptContext {
        ScriptContext {
            phase: ScriptPhase::PostResponse,
            response: Some(ScriptResponse {
                code: 201,
                status: "Created".into(),
                headers: vec![
                    ("Content-Type".into(), "application/json".into()),
                    ("X-Trace".into(), "abc".into()),
                ],
                body: body.into(),
                elapsed_millis: 42,
                size_bytes: body.len(),
            }),
            ..context()
        }
    }

    fn eval(source: &str) -> ScriptRun {
        run(source, context())
    }

    /// The run, asserted to have succeeded, so a failing test names the error
    /// rather than reporting a bare `None`.
    fn ok(source: &str) -> ScriptRun {
        let run = eval(source);
        assert!(run.error.is_none(), "script failed: {:?}", run.error);
        run
    }

    fn logs(run: &ScriptRun) -> Vec<String> {
        run.logs.iter().map(|entry| entry.message.clone()).collect()
    }

    // ---- console -------------------------------------------------------------

    #[test]
    fn console_captures_every_level_and_joins_its_arguments() {
        let run = ok(
            "console.debug('a'); console.log('b', 1); console.info('c'); \
             console.warn('d'); console.error('e');",
        );
        assert_eq!(logs(&run), ["a", "b 1", "c", "d", "e"]);
        let levels: Vec<ConsoleLevel> = run.logs.iter().map(|entry| entry.level).collect();
        assert_eq!(
            levels,
            [
                ConsoleLevel::Debug,
                ConsoleLevel::Log,
                // `info` is a `log`, as it is in a browser.
                ConsoleLevel::Log,
                ConsoleLevel::Warn,
                ConsoleLevel::Error,
            ]
        );
    }

    #[test]
    fn console_prints_objects_as_json_and_strings_as_themselves() {
        let run =
            ok("console.log({a: 1, b: [2]}); console.log('plain'); console.log(null, undefined);");
        assert_eq!(
            logs(&run),
            [r#"{"a":1,"b":[2]}"#, "plain", "null undefined"]
        );
    }

    // ---- pm.variables --------------------------------------------------------

    #[test]
    fn pm_variables_reads_the_merged_view_and_writes_run_locals() {
        let run = ok("console.log(pm.variables.get('host'));\
             console.log(pm.variables.get('version'));\
             console.log(String(pm.variables.has('nope')));\
             pm.variables.set('token', 'abc');\
             console.log(pm.variables.get('token'));\
             pm.variables.unset('token');\
             console.log(String(pm.variables.has('token')));");
        assert_eq!(logs(&run), ["example.com", "v1", "false", "abc", "false"]);
        // Unset removed it again, so nothing is carried out of the run.
        assert!(run.locals.is_empty());
        // A run-local write is never a persisted one.
        assert!(run.writes.is_empty());
    }

    #[test]
    fn a_missing_variable_reads_as_undefined_rather_than_throwing() {
        let run = ok("console.log(String(pm.variables.get('absent')));");
        assert_eq!(logs(&run), ["undefined"]);
    }

    // ---- pm.environment / pm.collectionVariables -----------------------------

    #[test]
    fn pm_environment_writes_are_reported_and_visible_to_the_script() {
        let run = ok("pm.environment.set('token', 'abc');\
             console.log(pm.environment.get('token'));\
             console.log(String(pm.environment.has('host')));");
        assert_eq!(logs(&run), ["abc", "true"]);
        assert_eq!(run.writes.len(), 1);
        assert_eq!(run.writes[0].scope, WriteScope::Environment);
        assert_eq!(run.writes[0].key, "token");
        assert_eq!(run.writes[0].value.as_deref(), Some("abc"));
        assert_eq!(
            run.environment.get("token").map(String::as_str),
            Some("abc")
        );
    }

    #[test]
    fn pm_environment_unset_reports_a_removal() {
        let run = ok("pm.environment.unset('host');");
        assert_eq!(run.writes.len(), 1);
        assert_eq!(run.writes[0].value, None);
        assert!(!run.environment.contains_key("host"));
    }

    #[test]
    fn pm_environment_to_object_hands_back_the_whole_scope() {
        let run = ok("console.log(JSON.stringify(pm.environment.toObject()));");
        assert_eq!(logs(&run), [r#"{"host":"example.com"}"#]);
    }

    #[test]
    fn pm_collection_variables_write_to_their_own_scope() {
        let run = ok("pm.collectionVariables.set('version', 'v2');");
        assert_eq!(run.writes[0].scope, WriteScope::Collection);
        assert_eq!(
            run.collection.get("version").map(String::as_str),
            Some("v2")
        );
        // The environment scope was not touched.
        assert_eq!(
            run.environment.get("host").map(String::as_str),
            Some("example.com")
        );
    }

    // ---- pm.request ----------------------------------------------------------

    #[test]
    fn a_script_that_reads_the_request_does_not_count_as_changing_it() {
        let run = ok("console.log(pm.request.method, pm.request.url.toString());");
        assert_eq!(logs(&run), ["GET https://example.com/things"]);
        assert!(
            run.request.is_none(),
            "reading must not trigger the write-back"
        );
    }

    #[test]
    fn pm_request_fields_are_writable() {
        let run = ok("pm.request.method = 'POST';\
             pm.request.url = 'https://example.com/other';\
             pm.request.body = '{\"a\":1}';");
        let request = run.request.expect("the request changed");
        assert_eq!(request.method, "POST");
        assert_eq!(request.url, "https://example.com/other");
        assert_eq!(request.body, "{\"a\":1}");
    }

    #[test]
    fn pm_request_headers_supports_the_five_postman_methods() {
        let run = ok("pm.request.headers.add({ key: 'X-One', value: '1' });\
             pm.request.headers.upsert({ key: 'accept', value: 'text/plain' });\
             pm.request.headers.upsert({ key: 'X-Two', value: '2' });\
             console.log(pm.request.headers.get('ACCEPT'));\
             console.log(String(pm.request.headers.has('x-one')));\
             pm.request.headers.remove('X-One');\
             console.log(String(pm.request.headers.all().length));");
        assert_eq!(logs(&run), ["text/plain", "true", "2"]);

        let request = run.request.expect("the headers changed");
        assert_eq!(
            request.headers,
            vec![
                // `upsert` matched case-insensitively and replaced in place.
                ("Accept".to_string(), "text/plain".to_string()),
                ("X-Two".to_string(), "2".to_string()),
            ]
        );
    }

    #[test]
    fn a_header_may_also_be_added_as_a_name_colon_value_string() {
        let run = ok("pm.request.headers.add('X-Token: abc');");
        let request = run.request.expect("the headers changed");
        assert!(request.headers.contains(&("X-Token".into(), "abc".into())));
    }

    // ---- pm.info, atob/btoa --------------------------------------------------

    #[test]
    fn pm_info_names_the_request_and_the_hook() {
        let run = ok("console.log(pm.info.requestName, pm.info.eventName);");
        assert_eq!(logs(&run), ["List things prerequest"]);
    }

    #[test]
    fn atob_and_btoa_round_trip() {
        let run = ok("console.log(btoa('hello')); console.log(atob('aGVsbG8='));");
        assert_eq!(logs(&run), ["aGVsbG8=", "hello"]);
    }

    #[test]
    fn btoa_refuses_a_character_outside_latin_1_the_way_a_browser_does() {
        let run = eval("btoa('déjà — vu');");
        assert!(matches!(run.error, Some(ScriptError::Threw { .. })));
    }

    #[test]
    fn atob_refuses_text_that_is_not_base64() {
        let run = eval("atob('!!!not base64!!!');");
        assert!(matches!(run.error, Some(ScriptError::Threw { .. })));
    }

    // ---- the shipped templates ----------------------------------------------

    #[test]
    fn every_shipped_pre_request_template_runs() {
        use crate::api_explorer::models::script_template::ScriptTemplate;

        for template in ScriptTemplate::PRE_REQUEST {
            let run = eval(template.snippet());
            assert!(
                run.error.is_none(),
                "a shipped template does not run: {:?}\n{}",
                run.error,
                template.snippet()
            );
        }
    }

    // ---- the sandbox ---------------------------------------------------------

    #[test]
    fn the_intrinsic_allowlist_leaves_out_what_it_says_it_does() {
        for absent in ["Promise", "Proxy", "WeakRef", "performance"] {
            let run = ok(&format!("console.log(typeof {absent});"));
            assert_eq!(
                logs(&run),
                ["undefined"],
                "{absent} is reachable but is not on the allowlist"
            );
        }
    }

    #[test]
    fn the_intrinsics_the_allowlist_does_name_are_all_there() {
        // Everything `report.md` §3.2 promises as ambient JavaScript.
        let run = ok(
            "console.log([Object, Array, String, Number, Boolean, Math, Error, Symbol, \
             Date, RegExp, JSON, Map, Set, Uint8Array].every(Boolean) ? 'all' : 'missing');",
        );
        assert_eq!(logs(&run), ["all"]);
    }

    #[test]
    fn there_is_no_timer_no_module_loader_and_no_process() {
        for absent in [
            "setTimeout",
            "setInterval",
            "require",
            "process",
            "globalThis.std",
            "globalThis.os",
            "fetch",
            "XMLHttpRequest",
            "WebSocket",
            "Deno",
        ] {
            let run = ok(&format!("console.log(typeof {absent});"));
            assert_eq!(logs(&run), ["undefined"], "{absent} exists in the sandbox");
        }
    }

    /// The hostile-script case, written as one script that tries every door.
    ///
    /// It runs through `eval` as well as directly, because `eval` **is**
    /// available (see this module's doc) and the claim being tested is that it
    /// buys an attacker nothing.
    #[test]
    fn a_hostile_script_cannot_reach_the_filesystem_a_process_or_the_network() {
        let attempts = [
            // Filesystem, by every name QuickJS's own libc layer would use.
            "std.open('/etc/passwd', 'r')",
            "os.open('/etc/passwd', 0)",
            "require('fs').readFileSync('/etc/passwd')",
            "new (require('fs').FileHandle)()",
            // A process.
            "os.exec(['/bin/sh', '-c', 'echo pwned'])",
            "std.popen('id', 'r')",
            "process.mainModule.require('child_process').execSync('id')",
            // The network, including the one dodo deliberately did not bind.
            "fetch('http://169.254.169.254/')",
            "new XMLHttpRequest()",
            "new WebSocket('ws://127.0.0.1:1/')",
            "pm.sendRequest({ url: 'http://169.254.169.254/' }, function () {})",
            // The environment of the host process.
            "std.getenv('HOME')",
            "process.env.HOME",
        ];

        for attempt in attempts {
            for source in [
                attempt.to_string(),
                // The same thing, built at runtime.
                format!("eval({:?})", attempt),
            ] {
                let run = run(&source, context());
                assert!(
                    run.error.is_some(),
                    "a hostile script succeeded: {source}\nlogs: {:?}",
                    logs(&run)
                );
                // Nothing escaped into the request or the variables either.
                assert!(run.request.is_none());
                assert!(run.writes.is_empty());
            }
        }
    }

    #[test]
    fn pm_send_request_fails_by_name_rather_than_as_an_opaque_type_error() {
        // `decision-pm-sendrequest-scope`: the binding is genuinely absent, and
        // the *reporting* is what makes the absence legible.
        let run = eval("pm.sendRequest({ url: 'http://example.com' });");
        assert_eq!(
            run.error,
            Some(ScriptError::Unsupported {
                name: "pm.sendRequest".into()
            })
        );
    }

    #[test]
    fn a_missing_timer_is_also_reported_by_name() {
        let run = eval("setTimeout(function () {}, 10);");
        assert_eq!(
            run.error,
            Some(ScriptError::Unsupported {
                name: "setTimeout".into()
            })
        );
    }

    // ---- bounds --------------------------------------------------------------

    #[test]
    fn an_infinite_loop_is_killed_by_the_deadline() {
        let started = std::time::Instant::now();
        let run = eval("while (true) {}");
        let elapsed = started.elapsed();

        assert_eq!(run.error, Some(ScriptError::Deadline { seconds: 2 }));
        assert!(
            elapsed < std::time::Duration::from_secs(6),
            "the deadline did not bound the run: {elapsed:?}"
        );
    }

    #[test]
    fn a_loop_that_allocates_is_stopped_too() {
        // A busy loop QuickJS can interrupt between allocations, rather than a
        // tight arithmetic one — the interrupt handler has to be polled on both.
        let run = eval("const a = []; while (true) { a.push({ x: a.length }); }");
        assert!(
            matches!(
                run.error,
                Some(ScriptError::Deadline { .. }) | Some(ScriptError::OutOfMemory)
            ),
            "an allocating loop was not bounded: {:?}",
            run.error
        );
    }

    #[test]
    fn the_memory_cap_holds() {
        // Well past the 16 MiB budget, in one allocation the engine must refuse.
        let run = eval("const big = new Array(64 * 1024 * 1024).fill(0);");
        assert!(
            matches!(
                run.error,
                Some(ScriptError::OutOfMemory) | Some(ScriptError::Deadline { .. })
            ),
            "the memory cap did not hold: {:?}",
            run.error
        );
    }

    #[test]
    fn the_stack_cap_holds() {
        let run = eval("function down(n) { return down(n + 1); } down(0);");
        assert!(
            matches!(
                run.error,
                Some(ScriptError::OutOfMemory) | Some(ScriptError::Threw { .. })
            ),
            "unbounded recursion was not stopped: {:?}",
            run.error
        );
    }

    #[test]
    fn a_wedged_run_does_not_poison_the_next_one() {
        // One fresh runtime per run is what this asserts: state cannot leak
        // between requests, tabs or collections.
        assert!(eval("while (true) {}").error.is_some());
        let run = ok("globalThis.leaked = 1; console.log(String(pm.variables.get('host')));");
        assert_eq!(logs(&run), ["example.com"]);

        let after = ok("console.log(typeof globalThis.leaked);");
        assert_eq!(logs(&after), ["undefined"]);
    }

    #[test]
    fn console_output_is_capped_and_the_overflow_is_counted() {
        let run = ok("for (let i = 0; i < 5000; i++) { console.log('line ' + i); }");
        assert_eq!(run.logs.len(), limits::CONSOLE_ENTRIES);
        assert!(
            run.dropped_logs > 0,
            "the cap dropped nothing but the loop logged 5000 lines"
        );
    }

    #[test]
    fn one_enormous_log_line_is_dropped_rather_than_taking_the_budget() {
        let run = ok("console.log('x'.repeat(200 * 1024)); console.log('after');");
        assert_eq!(logs(&run), ["after"]);
        assert_eq!(run.dropped_logs, 1);
    }

    #[test]
    fn a_variable_value_is_truncated_to_its_cap() {
        let run = ok("pm.environment.set('big', 'y'.repeat(1024 * 1024));");
        let value = run.writes[0].value.as_ref().expect("a value");
        assert_eq!(value.len(), limits::VARIABLE_VALUE_BYTES);
    }

    #[test]
    fn variable_writes_are_capped() {
        let run = ok("for (let i = 0; i < 1000; i++) { pm.environment.set('k' + i, String(i)); }");
        assert_eq!(run.writes.len(), limits::VARIABLE_WRITES);
    }

    // ---- failures ------------------------------------------------------------

    #[test]
    fn a_syntax_error_is_reported_with_the_engines_own_wording() {
        let run = eval("this is not javascript(");
        match run.error {
            Some(ScriptError::Threw { detail }) => {
                assert!(detail.contains("SyntaxError"), "unhelpful detail: {detail}")
            }
            other => panic!("expected a throw, got {other:?}"),
        }
    }

    #[test]
    fn a_run_keeps_what_it_logged_before_it_threw() {
        let run = eval("console.log('before'); null.boom;");
        assert!(run.error.is_some());
        assert_eq!(logs(&run), ["before"]);
    }

    #[test]
    fn the_engine_is_reachable_through_the_trait() {
        let run = QuickJsEngine.run("console.log('via the trait');", context());
        assert!(run.error.is_none());
        assert_eq!(logs(&run), ["via the trait"]);
    }

    // ---- pm.response ---------------------------------------------------------

    /// Runs `source` in the post-response phase against `body`, asserting the
    /// run itself succeeded.
    fn after(body: &str, source: &str) -> ScriptRun {
        let run = run(source, responded(body));
        assert!(run.error.is_none(), "script failed: {:?}", run.error);
        run
    }

    #[test]
    fn pm_response_reports_the_status_code_phrase_timing_and_size() {
        let run = after(
            "{}",
            "console.log(pm.response.code, pm.response.status, \
             pm.response.responseTime, pm.response.responseSize);",
        );
        assert_eq!(logs(&run), ["201 Created 42 2"]);
    }

    #[test]
    fn pm_response_text_and_json_read_the_body() {
        let run = after(
            "{\"id\":7,\"tags\":[\"a\"]}",
            "console.log(pm.response.text());\
             console.log(pm.response.json().id, pm.response.json().tags[0]);",
        );
        assert_eq!(logs(&run), ["{\"id\":7,\"tags\":[\"a\"]}", "7 a"]);
    }

    #[test]
    fn pm_response_json_throws_a_named_error_rather_than_undefined() {
        // Returning `undefined` would turn a wrong content type into a baffling
        // failure three lines later.
        let run = run("console.log(pm.response.json());", responded("not json"));
        match run.error {
            Some(ScriptError::Threw { detail }) => {
                assert!(detail.contains("JSONError"), "unhelpful detail: {detail}")
            }
            other => panic!("expected a throw, got {other:?}"),
        }
    }

    #[test]
    fn pm_response_headers_match_case_insensitively() {
        let run = after(
            "{}",
            "console.log(pm.response.headers.get('CONTENT-TYPE'));\
             console.log(String(pm.response.headers.has('x-trace')));\
             console.log(String(pm.response.headers.all().length));",
        );
        assert_eq!(logs(&run), ["application/json", "true", "2"]);
    }

    #[test]
    fn pm_response_does_not_exist_in_the_pre_request_phase() {
        // Absent, not an object full of zeroes: reaching for a response that
        // has not happened must fail rather than read as a status of 0.
        let run = ok("console.log(typeof pm.response);");
        assert_eq!(logs(&run), ["undefined"]);
    }

    #[test]
    fn pm_info_names_the_hook_it_is_running_in() {
        let run = after("{}", "console.log(pm.info.eventName);");
        assert_eq!(logs(&run), ["test"]);
    }

    #[test]
    fn a_post_response_script_cannot_change_the_request() {
        // The request has already gone; reading a change back would apply it to
        // nothing.
        let run = after("{}", "pm.request.url = 'https://elsewhere.example/';");
        assert!(run.request.is_none());
    }

    // ---- pm.test -------------------------------------------------------------

    #[test]
    fn a_passing_test_is_recorded_with_its_name_and_phase() {
        let run = after("{}", "pm.test('Status is 201', function () {});");
        assert_eq!(run.tests.len(), 1);
        assert_eq!(run.tests[0].name, "Status is 201");
        assert_eq!(run.tests[0].outcome, TestOutcome::Passed);
        assert_eq!(run.tests[0].phase, ScriptPhase::PostResponse);
    }

    #[test]
    fn a_failing_assertion_fails_that_test_and_lets_the_rest_run() {
        // The claim the Tests tab depends on: one bad assertion is a failed
        // row, not an aborted script.
        let run = after(
            "{}",
            "pm.test('one', function () { pm.expect(1).to.equal(2); });\
             pm.test('two', function () { pm.expect(1).to.equal(1); });\
             console.log('reached the end');",
        );
        assert_eq!(run.tests.len(), 2);
        match &run.tests[0].outcome {
            TestOutcome::Failed { message } => {
                assert!(message.contains("expected 1 to equal 2"), "{message}")
            }
            other => panic!("expected a failure, got {other:?}"),
        }
        assert_eq!(run.tests[1].outcome, TestOutcome::Passed);
        assert_eq!(logs(&run), ["reached the end"]);
    }

    #[test]
    fn a_throw_that_is_not_an_assertion_errors_the_test_rather_than_failing_it() {
        // Different words, different icon, different fix: the API is fine and
        // the script is wrong.
        let run = after("{}", "pm.test('boom', function () { null.id; });");
        match &run.tests[0].outcome {
            TestOutcome::Errored { message } => {
                assert!(message.contains("TypeError"), "{message}")
            }
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn pm_test_works_in_the_pre_request_phase_too() {
        let run = ok("pm.test('before', function () { pm.expect(1).to.be.ok; });");
        assert_eq!(run.tests.len(), 1);
        assert_eq!(run.tests[0].phase, ScriptPhase::PreRequest);
    }

    #[test]
    fn a_test_whose_body_is_not_a_function_errors_rather_than_passing_silently() {
        let run = after("{}", "pm.test('typo', 'oops');");
        assert!(matches!(run.tests[0].outcome, TestOutcome::Errored { .. }));
    }

    #[test]
    fn test_results_are_capped_and_the_overflow_is_counted() {
        let run = after(
            "{}",
            "for (let i = 0; i < 700; i++) { pm.test('t' + i, function () {}); }",
        );
        assert_eq!(run.tests.len(), limits::TEST_RESULTS);
        assert!(run.dropped_tests > 0);
    }

    #[test]
    fn tests_defined_before_a_throw_survive_it() {
        let run = run(
            "pm.test('first', function () {}); null.boom;",
            responded("{}"),
        );
        assert!(run.error.is_some());
        assert_eq!(run.tests.len(), 1);
    }

    // ---- pm.expect -----------------------------------------------------------

    /// Every matcher, asserted twice: the passing form and the failing one, so
    /// no matcher can quietly pass everything.
    #[test]
    fn every_matcher_the_surface_promises_both_passes_and_fails() {
        let cases: [(&str, &str); 14] = [
            ("pm.expect(1).to.equal(1)", "pm.expect(1).to.equal(2)"),
            (
                "pm.expect({a:[1]}).to.eql({a:[1]})",
                "pm.expect({a:[1]}).to.eql({a:[2]})",
            ),
            (
                "pm.expect('x').to.be.a('string')",
                "pm.expect('x').to.be.a('number')",
            ),
            ("pm.expect(true).to.be.true", "pm.expect(false).to.be.true"),
            (
                "pm.expect(false).to.be.false",
                "pm.expect(true).to.be.false",
            ),
            ("pm.expect(null).to.be.null", "pm.expect(0).to.be.null"),
            (
                "pm.expect(undefined).to.be.undefined",
                "pm.expect(0).to.be.undefined",
            ),
            ("pm.expect(1).to.be.ok", "pm.expect(0).to.be.ok"),
            (
                "pm.expect('abc').to.include('b')",
                "pm.expect('abc').to.include('z')",
            ),
            (
                "pm.expect('abc').to.match(/b/)",
                "pm.expect('abc').to.match(/z/)",
            ),
            (
                "pm.expect({id:1}).to.have.property('id', 1)",
                "pm.expect({id:1}).to.have.property('id', 2)",
            ),
            (
                "pm.expect([1,2]).to.have.lengthOf(2)",
                "pm.expect([1,2]).to.have.lengthOf(3)",
            ),
            ("pm.expect(5).to.be.above(1)", "pm.expect(5).to.be.above(9)"),
            (
                "pm.expect(5).to.be.within(1, 9)",
                "pm.expect(50).to.be.within(1, 9)",
            ),
        ];

        for (passes, fails) in cases {
            let run = after("{}", &format!("pm.test('t', function () {{ {passes}; }});"));
            assert_eq!(
                run.tests[0].outcome,
                TestOutcome::Passed,
                "should pass: {passes} — {:?}",
                run.tests[0].outcome
            );

            let run = after("{}", &format!("pm.test('t', function () {{ {fails}; }});"));
            assert!(
                matches!(run.tests[0].outcome, TestOutcome::Failed { .. }),
                "should fail: {fails} — {:?}",
                run.tests[0].outcome
            );
        }
    }

    #[test]
    fn below_is_the_mirror_of_above() {
        let run = after(
            "{}",
            "pm.test('a', function () { pm.expect(1).to.be.below(5); });\
             pm.test('b', function () { pm.expect(9).to.be.below(5); });",
        );
        assert_eq!(run.tests[0].outcome, TestOutcome::Passed);
        assert!(matches!(run.tests[1].outcome, TestOutcome::Failed { .. }));
    }

    #[test]
    fn not_negates_every_matcher_it_is_put_in_front_of() {
        let run = after(
            "{}",
            "pm.test('a', function () { pm.expect(1).to.not.equal(2); });\
             pm.test('b', function () { pm.expect(1).to.not.equal(1); });\
             pm.test('c', function () { pm.expect('abc').to.not.include('z'); });\
             pm.test('d', function () { pm.expect(true).to.not.be.true; });",
        );
        assert_eq!(run.tests[0].outcome, TestOutcome::Passed);
        assert!(matches!(run.tests[1].outcome, TestOutcome::Failed { .. }));
        assert_eq!(run.tests[2].outcome, TestOutcome::Passed);
        assert!(matches!(run.tests[3].outcome, TestOutcome::Failed { .. }));
    }

    #[test]
    fn a_failure_message_names_what_was_expected_and_what_was_there() {
        let run = after(
            "{}",
            "pm.test('t', function () { pm.expect({name: 'ada'}).to.have.property('id'); });",
        );
        match &run.tests[0].outcome {
            TestOutcome::Failed { message } => {
                assert!(message.contains("ada"), "{message}");
                assert!(message.contains("id"), "{message}");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    // ---- pm.response.to.* ----------------------------------------------------

    #[test]
    fn response_status_accepts_a_code_and_a_reason_phrase() {
        let run = after(
            "{}",
            "pm.test('a', function () { pm.response.to.have.status(201); });\
             pm.test('b', function () { pm.response.to.have.status('Created'); });\
             pm.test('c', function () { pm.response.to.have.status(200); });",
        );
        assert_eq!(run.tests[0].outcome, TestOutcome::Passed);
        assert_eq!(run.tests[1].outcome, TestOutcome::Passed);
        assert!(matches!(run.tests[2].outcome, TestOutcome::Failed { .. }));
    }

    #[test]
    fn response_header_json_and_json_body_all_assert() {
        let run = after(
            "{\"id\":7,\"items\":[{\"id\":\"x\"}]}",
            "pm.test('a', function () { pm.response.to.have.header('X-Trace'); });\
             pm.test('b', function () { pm.response.to.have.header('X-Absent'); });\
             pm.test('c', function () { pm.response.to.be.json; });\
             pm.test('d', function () { pm.response.to.have.jsonBody('id', 7); });\
             pm.test('e', function () { pm.response.to.have.jsonBody('items[0].id', 'x'); });\
             pm.test('f', function () { pm.response.to.have.jsonBody('nope'); });",
        );
        let outcomes: Vec<bool> = run
            .tests
            .iter()
            .map(|test| test.outcome == TestOutcome::Passed)
            .collect();
        assert_eq!(outcomes, [true, false, true, true, true, false]);
    }

    #[test]
    fn the_shipped_assert_status_template_runs_against_a_real_response() {
        use crate::api_explorer::models::script_template::ScriptTemplate;

        for template in ScriptTemplate::POST_RESPONSE {
            let run = run(template.snippet(), responded("{\"id\":7}"));
            assert!(
                run.error.is_none(),
                "a shipped template does not run: {:?}\n{}",
                run.error,
                template.snippet()
            );
        }
    }

    // ---- the syntax check ----------------------------------------------------

    #[test]
    fn a_script_that_parses_reports_nothing() {
        assert_eq!(check("pm.test('a', function () {});"), None);
        assert_eq!(check(""), None);
        assert_eq!(check("   \n  "), None);
    }

    #[test]
    fn a_syntax_error_is_found_with_its_line_and_a_message() {
        let found = check("const a = 1;\nconst b = ;\nconst c = 3;").expect("a syntax error");
        // 0-based, ready for the editor: the failure is on the second line.
        assert_eq!(found.line, 1, "{found:?}");
        assert!(
            found.detail.to_lowercase().contains("unexpected"),
            "unhelpful detail: {}",
            found.detail
        );
    }

    #[test]
    fn an_unclosed_block_is_reported_too() {
        let found = check("pm.test('a', function () {").expect("a syntax error");
        assert!(!found.detail.is_empty());
    }

    #[test]
    fn the_check_runs_nothing() {
        // If it evaluated, this would loop until the deadline and the assertion
        // below about elapsed time would fail.
        let started = std::time::Instant::now();
        assert_eq!(check("while (true) {}"), None);
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "the check executed the script"
        );
    }

    #[test]
    fn the_check_and_the_run_do_not_contradict_each_other() {
        // The property that matters: anything the check underlines must also
        // fail when run, because they are the same parser.
        for source in [
            "const a = ;",
            "function (",
            "pm.test('a', function () {",
            "}{",
        ] {
            assert!(check(source).is_some(), "the check stayed quiet: {source}");
            assert!(
                eval(source).error.is_some(),
                "the engine ran what the check rejected: {source}"
            );
        }
    }

    #[test]
    fn a_stack_frame_yields_a_line_and_a_column() {
        assert_eq!(position("    at dodo-script:12:5\n"), Some((12, 5)));
        assert_eq!(position("    at dodo-script:3\n"), Some((3, 1)));
        assert_eq!(position("nothing useful"), None);
    }

    #[test]
    fn the_engines_check_is_reachable_through_the_trait() {
        assert!(QuickJsEngine.check("const a = ;").is_some());
        assert!(QuickJsEngine.check("const a = 1;").is_none());
    }
}

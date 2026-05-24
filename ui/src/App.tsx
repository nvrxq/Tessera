import { createSignal, Show, type Component } from "solid-js";
import Terminal from "./Terminal";

const HOME = "/home/save";

const App: Component = () => {
  const [open, setOpen] = createSignal(false);

  return (
    <>
      <header>
        <h1>Tessera</h1>
        <Show
          when={open()}
          fallback={
            <button type="button" onClick={() => setOpen(true)}>
              Spawn shell in {HOME}
            </button>
          }
        >
          <button type="button" onClick={() => setOpen(false)}>
            Close terminal
          </button>
        </Show>
      </header>
      <Show when={open()}>
        <Terminal cwd={HOME} />
      </Show>
    </>
  );
};

export default App;

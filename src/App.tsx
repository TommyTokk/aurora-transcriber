import { AudioLines } from "lucide-react";

/** Renders the initial application shell without starting audio processing. */
export default function App() {
  return (
    <main className="flex min-h-screen items-center justify-center bg-slate-950 px-8 text-slate-100">
      <section className="max-w-xl text-center">
        <AudioLines
          aria-hidden="true"
          className="mx-auto mb-6 size-12 text-teal-400"
        />
        <h1 className="text-4xl font-semibold tracking-tight">AURORA</h1>
        <p className="mt-4 text-lg text-slate-300">
          La tua voce, sul tuo dispositivo.
        </p>
        <p className="mt-3 text-sm text-slate-400">
          Trascrizione locale e privata. L’interfaccia di trascrizione sarà
          disponibile nelle prossime milestone.
        </p>
      </section>
    </main>
  );
}

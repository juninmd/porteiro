import { useEffect, useState } from "react";
import { api } from "../api";
import { shotKind } from "../format";

/** Loads one capture through the backend (the webview has no file access) and frees the blob URL on unmount. */
export function ShotImage({ name, className }: { name: string; className?: string }) {
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let live = true;
    let created: string | null = null;
    api
      .shot(name)
      .then((buf) => {
        created = URL.createObjectURL(new Blob([buf], { type: "image/jpeg" }));
        if (live) setUrl(created);
        else URL.revokeObjectURL(created);
      })
      .catch(() => live && setFailed(true));
    return () => {
      live = false;
      if (created) URL.revokeObjectURL(created);
    };
  }, [name]);

  const label = shotKind(name) === "cam" ? "Foto da webcam" : "Captura de tela";
  if (failed)
    return <div className={`grid place-items-center bg-raised text-xs text-mute ${className}`}>indisponível</div>;
  if (!url)
    return (
      <div
        className={`animate-pulse motion-reduce:animate-none bg-raised ${className}`}
        role="img"
        aria-label={`${label} carregando`}
      />
    );
  return <img src={url} alt={label} className={className} />;
}

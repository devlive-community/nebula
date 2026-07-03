import { breadcrumbs } from "../util";

interface Props {
  path: string;
  onNavigate: (path: string) => void;
}

export function Breadcrumb({ path, onNavigate }: Props) {
  const crumbs = breadcrumbs(path);
  return (
    <div className="breadcrumb">
      {crumbs.map((c, i) => (
        <span key={c.path} className="breadcrumb__item">
          <button className="breadcrumb__link" onClick={() => onNavigate(c.path)}>
            {c.label}
          </button>
          {i < crumbs.length - 1 && <span className="breadcrumb__sep">/</span>}
        </span>
      ))}
    </div>
  );
}

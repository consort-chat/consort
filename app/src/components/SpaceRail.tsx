import { HOME_ID, type Space } from "../lib/api";
import { RoomAvatar } from "./RoomAvatar";
import "./SpaceRail.css";

/** The Home glyph. A house, because that is what Home has looked like since. */
function HomeIcon() {
  return (
    <svg
      className="rail__glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M3 10.5 12 3l9 7.5" />
      <path d="M5.5 9.5V20h13V9.5" />
      <path d="M9.75 20v-5.5h4.5V20" />
    </svg>
  );
}

interface Props {
  spaces: Space[];
  selectedId: string;
  onSelect: (id: string) => void;
}

/**
 * One icon per joined space, and a Home button for everything else.
 *
 * Home is always first and never sorts with the others. It is furniture: a
 * rail whose first entry moves depending on whether somebody joined a space
 * beginning with an earlier letter is a rail that cannot be used from muscle
 * memory.
 *
 * The order of everything after it is decided in Rust, and this does not
 * re-sort. Two places deciding the same order is two places to disagree.
 */
export function SpaceRail({ spaces, selectedId, onSelect }: Props) {
  return (
    <nav className="rail" aria-label="Spaces">
      <ul className="rail__list">
        {spaces.map((space) => {
          const home = space.id === HOME_ID;
          const selected = space.id === selectedId;

          return (
            <li key={space.id}>
              <button
                type="button"
                className="rail__entry"
                data-selected={selected}
                data-home={home}
                /*
                  The name is here rather than in the markup because the
                  content is a picture or an initial, neither of which is what
                  the space is called. `title` gives the same string as a
                  tooltip, which is what a rail of wordless icons needs.
                */
                aria-label={space.name}
                aria-current={selected ? "true" : undefined}
                title={space.name}
                onClick={() => onSelect(space.id)}
              >
                <span className="rail__pill" aria-hidden="true" />
                {home ? (
                  <span className="rail__home">
                    <HomeIcon />
                  </span>
                ) : (
                  <RoomAvatar
                    roomId={space.id}
                    name={space.name}
                    avatar={space.avatar}
                    className="rail__avatar"
                  />
                )}
              </button>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}

import { opendir } from "fs/promises";
import path from "path";
import React from "react";
import { Trans, useTranslation } from "react-i18next";
import AutoSizer from "react-virtualized-auto-sizer";
import { FixedSizeList, ListChildComponentProps } from "react-window";

import { shell } from "@electron/remote";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import VideoFileOutlinedIcon from "@mui/icons-material/VideoFileOutlined";
import Box from "@mui/material/Box";
import IconButton from "@mui/material/IconButton";
import ListItem from "@mui/material/ListItem";
import ListItemText from "@mui/material/ListItemText";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";

import { getReplaysPath } from "../../../paths";
import { readReplayMetadata, ReplayInfo } from "../../../replay";

async function* walk(dir: string, root?: string): AsyncIterable<string> {
  if (root == null) {
    root = dir;
  }
  for await (const d of await opendir(dir)) {
    const entry = path.join(dir, d.name);
    if (d.isDirectory()) {
      yield* await walk(entry, root);
    } else if (d.isFile()) {
      yield path.relative(root, entry);
    }
  }
}

function ReplayItem({
  ListChildProps: { index, style },
  replay,
}: {
  ListChildProps: ListChildComponentProps;
  replay: { name: string; info: ReplayInfo };
}) {
  const { i18n } = useTranslation();
  const dateFormat = new Intl.DateTimeFormat(i18n.resolvedLanguage, {
    dateStyle: "medium",
    timeStyle: "medium",
  });
  return (
    <ListItem
      style={style}
      key={index}
      sx={{ userSelect: "none" }}
      secondaryAction={
        <Stack direction="row">
          <Tooltip title={<Trans i18nKey="replays:show-file" />}>
            <IconButton
              onClick={() => {
                shell.showItemInFolder(
                  path.join(getReplaysPath(), replay.name)
                );
              }}
            >
              <FolderOpenIcon />
            </IconButton>
          </Tooltip>
          <Tooltip title={<Trans i18nKey="replays:export-video" />}>
            <IconButton>
              <VideoFileOutlinedIcon />
            </IconButton>
          </Tooltip>
          <Tooltip title={<Trans i18nKey="replays:play" />}>
            <IconButton>
              <PlayArrowIcon />
            </IconButton>
          </Tooltip>
        </Stack>
      }
    >
      <ListItemText
        primary={dateFormat.format(new Date(replay.info.ts))}
        secondary={replay.name}
      />
    </ListItem>
  );
}

export default function ReplaysPane({ active }: { active: boolean }) {
  const [replays, setReplays] = React.useState<
    { name: string; info: ReplayInfo }[] | null
  >(null);

  React.useEffect(() => {
    if (!active) {
      setReplays(null);
      return;
    }

    (async () => {
      const replays = [];
      try {
        for await (const filename of walk(getReplaysPath())) {
          let replayInfo = null;
          try {
            replayInfo = await readReplayMetadata(
              path.join(getReplaysPath(), filename)
            );
          } catch (e) {
            console.error("failed to get replay data for %s:", filename, e);
          }
          if (replayInfo == null) {
            continue;
          }
          replays.push({ name: filename, info: replayInfo });
        }
      } catch (e) {
        console.error("failed to get replays:", e);
      }
      replays.sort(({ name: name1 }, { name: name2 }) => {
        return name1 < name2 ? -1 : name1 > name2 ? 1 : 0;
      });
      replays.reverse();
      setReplays(replays);
    })();
  }, [active]);

  return (
    <Box
      sx={{
        width: "100%",
        height: "100%",
        display: active ? "block" : "none",
      }}
    >
      {replays != null ? (
        <AutoSizer>
          {({ height, width }) => (
            <FixedSizeList
              height={height}
              width={width}
              itemCount={replays.length}
              itemSize={60}
            >
              {(props) => (
                <ReplayItem
                  ListChildProps={props}
                  replay={replays[props.index]}
                />
              )}
            </FixedSizeList>
          )}
        </AutoSizer>
      ) : null}
    </Box>
  );
}

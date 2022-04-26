import { opendir } from "fs/promises";
import path from "path";
import React from "react";
import { Trans } from "react-i18next";
import AutoSizer from "react-virtualized-auto-sizer";
import { FixedSizeList, ListChildComponentProps } from "react-window";

import FolderOutlinedIcon from "@mui/icons-material/FolderOutlined";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import VideoFileOutlinedIcon from "@mui/icons-material/VideoFileOutlined";
import Box from "@mui/material/Box";
import IconButton from "@mui/material/IconButton";
import ListItem from "@mui/material/ListItem";
import ListItemText from "@mui/material/ListItemText";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";

import { getReplaysPath } from "../../../paths";

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
  replayName,
}: {
  ListChildProps: ListChildComponentProps;
  replayName: string;
}) {
  return (
    <ListItem
      style={style}
      key={index}
      sx={{ userSelect: "none" }}
      secondaryAction={
        <Stack direction="row">
          <Tooltip title={<Trans i18nKey="replays:show-file" />}>
            <IconButton>
              <FolderOutlinedIcon />
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
      <ListItemText primary={replayName} secondary="bottom text" />
    </ListItem>
  );
}

export default function ReplaysPane({ active }: { active: boolean }) {
  const [replayNames, setReplayNames] = React.useState<string[] | null>(null);

  React.useEffect(() => {
    if (!active) {
      setReplayNames(null);
      return;
    }

    (async () => {
      const names = [];
      for await (const entry of walk(getReplaysPath())) {
        names.push(entry);
      }
      names.sort();
      names.reverse();
      setReplayNames(names);
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
      {replayNames != null ? (
        <AutoSizer>
          {({ height, width }) => (
            <FixedSizeList
              height={height}
              width={width}
              itemCount={replayNames.length}
              itemSize={60}
            >
              {(props) => (
                <ReplayItem
                  ListChildProps={props}
                  replayName={replayNames[props.index]}
                />
              )}
            </FixedSizeList>
          )}
        </AutoSizer>
      ) : null}
    </Box>
  );
}

import React from "react";
import { Trans } from "react-i18next";

import BrowserNotSupportedIcon from "@mui/icons-material/BrowserNotSupported";
import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import Tab from "@mui/material/Tab";
import Tabs from "@mui/material/Tabs";
import Typography from "@mui/material/Typography";

import { Editor } from "../../saveedit";
import * as bn4 from "../../saveedit/bn4";
import * as bn6 from "../../saveedit/bn6";
import FolderViewer from "./FolderViewer";
import ModcardsViewer from "./ModcardsViewer";
import NavicustViewer from "./NavicustViewer";

function BN4SaveViewer({ editor }: { editor: bn4.Editor }) {
  const [tab, setTab] = React.useState("folder");

  return (
    <Stack flexGrow={1} flexShrink={0}>
      <Tabs
        sx={{ px: 1 }}
        value={tab}
        onChange={(e, value) => {
          setTab(value);
        }}
      >
        <Tab label={<Trans i18nKey="play:tab.folder" />} value="folder" />
      </Tabs>
      <FolderViewer editor={editor} active={tab == "folder"} />
    </Stack>
  );
}

function BN6SaveViewer({ editor }: { editor: bn6.Editor }) {
  const [tab, setTab] = React.useState("navicust");

  React.useEffect(() => {
    if (tab == "modcards" && !editor.supportsModcards()) {
      setTab("navicust");
    }
  }, [tab, editor]);

  return (
    <Stack flexGrow={1} flexShrink={0}>
      <Tabs
        sx={{ px: 1 }}
        value={tab}
        onChange={(e, value) => {
          setTab(value);
        }}
      >
        <Tab label={<Trans i18nKey="play:tab.navicust" />} value="navicust" />
        <Tab label={<Trans i18nKey="play:tab.folder" />} value="folder" />
        <Tab
          label={<Trans i18nKey="play:tab.modcards" />}
          value="modcards"
          disabled={!editor.supportsModcards()}
        />
      </Tabs>
      <NavicustViewer editor={editor} active={tab == "navicust"} />
      <FolderViewer editor={editor} active={tab == "folder"} />
      {editor.supportsModcards() ? (
        <ModcardsViewer editor={editor} active={tab == "modcards"} />
      ) : null}
    </Stack>
  );
}

export default function SaveViewer({ editor }: { editor: Editor }) {
  switch (editor.getGameFamily()) {
    case "bn6":
      return <BN6SaveViewer editor={editor as bn6.Editor} />;
    case "bn4":
      return <BN4SaveViewer editor={editor as bn4.Editor} />;
    default:
      return (
        <Box
          flexGrow={1}
          display="flex"
          justifyContent="center"
          alignItems="center"
          sx={{ userSelect: "none", color: "text.disabled" }}
        >
          <Stack alignItems="center" spacing={1}>
            <BrowserNotSupportedIcon sx={{ fontSize: "4rem" }} />
            <Typography variant="h6">
              <Trans i18nKey="play:save-viewer-unsupported" />
            </Typography>
          </Stack>
        </Box>
      );
  }
}

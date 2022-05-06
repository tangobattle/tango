import React from "react";
import { Trans } from "react-i18next";

import Stack from "@mui/material/Stack";
import Tab from "@mui/material/Tab";
import Tabs from "@mui/material/Tabs";

import * as bn6 from "../../saveedit/bn6";
import FolderViewer from "./FolderViewer";
import ModcardsViewer from "./ModcardsViewer";
import NavicustViewer from "./NavicustViewer";

export default function SaveViewer({ editor }: { editor: bn6.Editor }) {
  const [tab, setTab] = React.useState("navicust");

  React.useEffect(() => {
    if (tab == "modcards" && !editor.supportsModcards()) {
      setTab("navicust");
    }
  }, [tab, editor]);

  return (
    <Stack flexGrow={1} flexShrink={0} sx={{ width: 0 }}>
      <Tabs
        sx={{ px: 1 }}
        value={tab}
        onChange={(e, value) => {
          setTab(value);
        }}
      >
        <Tab label={<Trans i18nKey="saves:tab.navicust" />} value="navicust" />
        <Tab label={<Trans i18nKey="saves:tab.folder" />} value="folder" />
        <Tab
          label={<Trans i18nKey="saves:tab.modcards" />}
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

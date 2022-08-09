import React from "react";
import { Trans } from "react-i18next";

import BrowserNotSupportedIcon from "@mui/icons-material/BrowserNotSupported";
import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import Tab from "@mui/material/Tab";
import Tabs from "@mui/material/Tabs";
import Typography from "@mui/material/Typography";

import { Editor } from "../../../saveedit";
import BN4ModcardsViewer from "./BN4ModcardsViewer";
import DarkAIViewer from "./DarkAIViewer";
import FolderViewer, { AllowEdits as AllowFolderEdits } from "./FolderViewer";
import ModcardsViewer from "./ModcardsViewer";
import NavicustViewer from "./NavicustViewer";
import NaviViewer from "./NaviViewer";

export default function SaveViewer({
  editor,
  allowFolderEdits,
}: {
  editor: Editor;
  allowFolderEdits: AllowFolderEdits;
}) {
  const naviEditor = editor.getNaviEditor();
  const navicustEditor = editor.getNavicustEditor();
  const folderEditor = editor.getFolderEditor();
  const modcardsEditor = editor.getModcardsEditor();
  const bn4ModcardsEditor = editor.getBN4ModcardsEditor();
  const darkAIEditor = editor.getDarkAIEditor();

  const availableTabs = React.useMemo(
    () => [
      ...(naviEditor != null ? ["navi"] : []),
      ...(navicustEditor != null ? ["navicust"] : []),
      ...(folderEditor != null ? ["folder"] : []),
      ...(modcardsEditor != null || bn4ModcardsEditor != null
        ? ["modcards"]
        : []),
      ...(darkAIEditor != null ? ["darkai"] : []),
    ],
    [
      naviEditor,
      navicustEditor,
      folderEditor,
      modcardsEditor,
      bn4ModcardsEditor,
      darkAIEditor,
    ]
  );

  const [tab, setTab] = React.useState(availableTabs[0]);

  React.useEffect(() => {
    if (availableTabs.indexOf(tab) == -1) {
      setTab(availableTabs[0]);
    }
  }, [tab, availableTabs]);

  return (
    <>
      {availableTabs.length > 0 ? (
        <Stack flexGrow={1} flexShrink={0}>
          <Tabs
            sx={{ px: 1 }}
            value={tab}
            onChange={(e, value) => {
              setTab(value);
            }}
          >
            {naviEditor != null ? (
              <Tab label={<Trans i18nKey="play:tab.navi" />} value="navi" />
            ) : null}
            {navicustEditor != null ? (
              <Tab
                label={<Trans i18nKey="play:tab.navicust" />}
                value="navicust"
              />
            ) : null}
            {folderEditor != null ? (
              <Tab label={<Trans i18nKey="play:tab.folder" />} value="folder" />
            ) : null}
            {modcardsEditor != null ? (
              <Tab
                label={<Trans i18nKey="play:tab.modcards" />}
                value="modcards"
              />
            ) : null}
            {bn4ModcardsEditor != null ? (
              <Tab
                label={<Trans i18nKey="play:tab.modcards" />}
                value="modcards"
              />
            ) : null}
            {darkAIEditor != null ? (
              <Tab label={<Trans i18nKey="play:tab.darkai" />} value="darkai" />
            ) : null}
          </Tabs>
          {naviEditor != null ? (
            <NaviViewer editor={naviEditor} active={tab == "navi"} />
          ) : null}
          {navicustEditor != null ? (
            <NavicustViewer
              romName={editor.getROMInfo().name}
              editor={navicustEditor}
              active={tab == "navicust"}
            />
          ) : null}
          {folderEditor != null ? (
            <FolderViewer
              allowEdits={allowFolderEdits}
              editor={folderEditor}
              active={tab == "folder"}
            />
          ) : null}
          {modcardsEditor != null ? (
            <ModcardsViewer
              editor={modcardsEditor}
              active={tab == "modcards"}
            />
          ) : null}
          {bn4ModcardsEditor != null ? (
            <BN4ModcardsViewer
              editor={bn4ModcardsEditor}
              active={tab == "modcards"}
            />
          ) : null}
          {darkAIEditor != null && folderEditor != null ? (
            <DarkAIViewer
              editor={darkAIEditor}
              folderEditor={folderEditor}
              active={tab == "darkai"}
            />
          ) : null}
        </Stack>
      ) : (
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
      )}
    </>
  );
}

import React from "react";
import { useTranslation } from "react-i18next";

import HomeIcon from "@mui/icons-material/Home";
import HomeOutlinedIcon from "@mui/icons-material/HomeOutlined";
import LibraryBooksIcon from "@mui/icons-material/LibraryBooks";
import LibraryBooksOutlinedIcon from "@mui/icons-material/LibraryBooksOutlined";
import SettingsIcon from "@mui/icons-material/Settings";
import SettingsOutlinedIcon from "@mui/icons-material/SettingsOutlined";
import SlowMotionVideoIcon from "@mui/icons-material/SlowMotionVideo";
import SlowMotionVideoOutlinedIcon from "@mui/icons-material/SlowMotionVideoOutlined";
import Badge from "@mui/material/Badge";
import MuiDrawer from "@mui/material/Drawer";
import List from "@mui/material/List";
import ListItemButton from "@mui/material/ListItemButton";
import ListItemIcon from "@mui/material/ListItemIcon";
import { styled } from "@mui/material/styles";
import Tooltip from "@mui/material/Tooltip";

import { useUpdateStatus } from "./UpdaterStatusContext";

const Drawer = styled(MuiDrawer, {
  shouldForwardProp: (prop) => prop !== "open",
})(({ theme }) => ({
  flexShrink: 0,
  whiteSpace: "nowrap",
  boxSizing: "border-box",
  overflowX: "hidden",
  width: `calc(${theme.spacing(6)} + 1px)`,
}));

function NavbarButton({
  title,
  onClick,
  unselectedIcon,
  selectedIcon,
  selected,
}: {
  title: string;
  onClick: React.MouseEventHandler;
  unselectedIcon: React.ReactNode;
  selectedIcon: React.ReactNode;
  selected?: boolean;
}) {
  return (
    <Tooltip title={title} enterDelay={0} placement="right">
      <ListItemButton
        onClick={onClick}
        selected={selected}
        sx={{
          minHeight: 48,
          px: 1.5,
          justifyContent: "center",
        }}
      >
        <ListItemIcon
          sx={{
            minWidth: 0,
            mr: "auto",
            justifyContent: "center",
          }}
        >
          {selected ? selectedIcon : unselectedIcon}
        </ListItemIcon>
      </ListItemButton>
    </Tooltip>
  );
}

export type NavbarSelection = "home" | "saves" | "replays" | "settings" | null;

export default function Navbar({
  selected,
  onSelect,
}: {
  selected: NavbarSelection;
  onSelect: (selected: NavbarSelection) => void;
}) {
  const { t } = useTranslation();
  const { status: updateStatus } = useUpdateStatus();

  const SettingsIconWrapper = ({ children }: { children: React.ReactNode }) =>
    updateStatus == "available" ? (
      <Badge color="secondary" variant="dot">
        {children}
      </Badge>
    ) : updateStatus == "downloaded" ? (
      <Badge color="primary" variant="dot">
        {children}
      </Badge>
    ) : (
      <>{children}</>
    );

  return (
    <Drawer variant="permanent" open={true}>
      <List>
        <NavbarButton
          selected={selected == "home"}
          onClick={() => {
            onSelect("home");
          }}
          title={t("navbar:home")}
          unselectedIcon={<HomeOutlinedIcon />}
          selectedIcon={<HomeIcon />}
        />
        <NavbarButton
          selected={selected == "saves"}
          onClick={() => {
            onSelect("saves");
          }}
          title={t("navbar:saves")}
          unselectedIcon={<LibraryBooksOutlinedIcon />}
          selectedIcon={<LibraryBooksIcon />}
        />
        <NavbarButton
          selected={selected == "replays"}
          onClick={() => {
            onSelect("replays");
          }}
          title={t("navbar:replays")}
          unselectedIcon={<SlowMotionVideoOutlinedIcon />}
          selectedIcon={<SlowMotionVideoIcon />}
        />
      </List>
      <List style={{ marginTop: "auto" }}>
        <NavbarButton
          selected={selected == "settings"}
          onClick={() => {
            onSelect("settings");
          }}
          title={
            updateStatus == "available"
              ? t("navbar:settings-update-available")
              : updateStatus == "downloaded"
              ? t("navbar:settings-update-downloaded")
              : t("navbar:settings")
          }
          unselectedIcon={
            <SettingsIconWrapper>
              <SettingsOutlinedIcon />
            </SettingsIconWrapper>
          }
          selectedIcon={
            <SettingsIconWrapper>
              <SettingsIcon />
            </SettingsIconWrapper>
          }
        />
      </List>
    </Drawer>
  );
}

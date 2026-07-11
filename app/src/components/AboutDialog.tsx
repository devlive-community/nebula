import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faXmark } from "@fortawesome/free-solid-svg-icons";
import { Logo } from "./Logo";
import { useI18n } from "../i18n";

interface Props {
  onClose: () => void;
}

/** 自定义"关于"弹窗,替代系统默认关于面板。 */
export function AboutDialog({ onClose }: Props) {
  const { t } = useI18n();
  const [version, setVersion] = useState("");

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(""));
  }, []);

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal modal--about"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal__header">
          <h3>{t("关于")}</h3>
          <button className="modal__close" onClick={onClose}>
            <FontAwesomeIcon icon={faXmark} />
          </button>
        </div>

        <div className="modal__body about">
          <div className="about__brand">
            <Logo size={56} />
            <div className="about__name">Nebula</div>
            {version && <div className="about__version">v{version}</div>}
          </div>

          <p className="about__desc">
            {t(
              "跨平台桌面端多云对象存储管理器,用统一界面管理阿里云 OSS、华为云 OBS 等多家云对象存储:浏览、上传下载、分享、传输管理,一站搞定。",
            )}
          </p>

          <div className="about__links">
            <span className="about__link">nebula.devlive.org</span>
            <span className="about__sep">·</span>
            <span className="about__link">
              github.com/devlive-community/nebula
            </span>
          </div>

          <div className="about__meta">
            <span>MIT License</span>
            <span>© 2026 Devlive Community</span>
          </div>
        </div>
      </div>
    </div>
  );
}

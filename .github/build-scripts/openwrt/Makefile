include $(TOPDIR)/rules.mk

PKG_NAME:=telomere-cli
PKG_VERSION:=0.0.1
PKG_RELEASE:=1
#RUST_PKG_FEATURES:=

SOURCE_DIR:=./files/telomere
PKG_BUILD_DIR:=$(BUILD_DIR)/$(PKG_NAME)

PKG_BUILD_DEPENDS:=rust/host
PKG_BUILD_PARALLEL:=1
LDLIBS:=-latomic

include $(INCLUDE_DIR)/package.mk
include $(TOPDIR)/feeds/packages/lang/rust/rust-package.mk

define Package/telomere-cli
	SECTION:=utils
	CATEGORY:=telomere
	TITLE:=Telomere Cli
	DEPENDS:=$(RUST_ARCH_DEPENDS) +libatomic
endef

define Package/telomere-cli/description
	Telomere Runtime Cli
endef

define Build/Prepare
	mkdir -p $(PKG_BUILD_DIR)
	cp -r $(SOURCE_DIR)/* $(PKG_BUILD_DIR)/
endef

define Package/telomere/install
	$(INSTALL_DIR) $(1)/usr/bin
	$(INSTALL_BIN) $(PKG_INSTALL_DIR)/bin/telomere-cli $(1)/usr/bin/
endef

$(eval $(call RustBinPackage,telomere-cli))
$(eval $(call BuildPackage,telomere-cli))
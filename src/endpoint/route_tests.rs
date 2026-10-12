use super::pick;

fn file(text: &str) -> crate::config::File {
    toml::from_str(text).unwrap()
}

fn base(file: &crate::config::File, name: &str) -> String {
    let route = pick(file, name);
    super::super::resolve::seat(file, &route)
        .and_then(|endpoint| endpoint.base_url)
        .unwrap_or_default()
}

#[test]
fn a_provider_toggle_picks_the_route_for_its_region_and_access() {
    let china = file("[provider.zai]\nregion = \"cn\"\naccess = \"plan\"\n");
    assert_eq!(
        base(&china, "zai"),
        "https://open.bigmodel.cn/api/coding/paas/v4"
    );
    let key = file("[provider.zai]\nregion = \"intl\"\naccess = \"key\"\n");
    assert_eq!(base(&key, "zai"), "https://api.z.ai/api/paas/v4");
    let plain = file("");
    assert_eq!(base(&plain, "zai"), "https://api.z.ai/api/coding/paas/v4");
}

#[test]
fn a_route_with_a_key_wins_when_no_toggle_is_set() {
    let keyed = file("[endpoint.zai-cn]\napi_key = \"k\"\n");
    assert_eq!(pick(&keyed, "zai"), "zai-cn");
}

#[test]
fn old_row_names_still_reach_their_route_unless_the_user_owns_the_name() {
    let plain = file("");
    assert_eq!(pick(&plain, "glm"), "zai-cn");
    assert_eq!(pick(&plain, "glm-intl"), "zai");
    assert_eq!(pick(&plain, "zai-cn"), "zai-cn");
    assert_eq!(pick(&plain, "zen"), "opencode");
    let owned = file("[endpoint.glm]\nbase_url = \"http://127.0.0.1:9/v1\"\nmodel = \"m\"\n");
    assert_eq!(pick(&owned, "glm"), "glm");
}

#[test]
fn the_provider_list_names_each_provider_once() {
    let names = super::provider_names(&file("[endpoint.zai-cn]\nmodel = \"glm-5.3\"\n"));
    for provider in [
        "zai",
        "alibaba",
        "minimax",
        "siliconflow",
        "anthropic",
        "opencode",
        "kimi",
    ] {
        assert_eq!(
            names.iter().filter(|name| *name == provider).count(),
            1,
            "{provider} in {names:?}"
        );
    }
    for route in [
        "zai-cn",
        "glm",
        "glm-intl",
        "qwen-plan",
        "dashscope-cn",
        "minimax-cn-plan",
        "claude",
        "zen",
        "go",
    ] {
        assert!(
            !names.iter().any(|name| name == route),
            "{route} in {names:?}"
        );
    }
}

#[test]
fn a_toggle_switches_one_axis_and_keeps_the_other() {
    let mut info = super::super::auth_info("zai-cn").unwrap();
    assert_eq!(info.provider, "zai");
    info.name = "zai-cn".to_string();
    let region = super::next(&info, super::Toggle::Region).unwrap();
    assert_eq!(region.name, "zai");
    let access = super::next(&info, super::Toggle::Access).unwrap();
    assert_eq!(access.name, "zai-cn-api");
    assert_eq!(info.title(), "zai");
}

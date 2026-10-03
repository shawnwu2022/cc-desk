//! Actual protected package namespace probes. These never fabricate terminal
//! process/job custody or admit a production ReturnBoundary.
use super::*;

// 保留包位于事务的 package 子目录；父事务、其他事务和同级目录不能替代。
#[test]
fn HistoryReturnBoundary_ExactPackageChild_001() {
    let temporary = tempfile::tempdir().unwrap();
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let data =
        PrivateDirectory::create_new(parent.clone(), name("transaction").unwrap(), &user).unwrap();
    let package =
        PrivateDirectory::create_new(data.directory().clone(), name("package").unwrap(), &user)
            .unwrap();
    assert_ne!(package.directory().identity(), data.directory().identity());
    verify_package_child(&package, &data, &user).unwrap();
    assert!(verify_package_child(&data, &data, &user).is_err());

    let other =
        PrivateDirectory::create_new(data.directory().clone(), name("other").unwrap(), &user)
            .unwrap();
    assert!(verify_package_child(&other, &data, &user).is_err());
    let foreign =
        PrivateDirectory::create_new(parent, name("foreign-transaction").unwrap(), &user).unwrap();
    let foreign_package =
        PrivateDirectory::create_new(foreign.directory().clone(), name("package").unwrap(), &user)
            .unwrap();
    assert!(verify_package_child(&foreign_package, &data, &user).is_err());
    verify_package_child(&foreign_package, &foreign, &user).unwrap();
    verify_package_child(&package, &data, &user).unwrap();
}

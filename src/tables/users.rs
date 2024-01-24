use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use chrono::NaiveDateTime;
use uuid::Uuid;

use subseq_util::tables::{UserTable, ValidationErrorMessage};

subseq_util::create_user_base!();
crate::zini_table!(UserMetadata, crate::schema::auth::metadata::dsl::metadata);
crate::zini_table!(UserPortraits, crate::schema::auth::portraits::dsl::portraits);
crate::zini_table!(UserIdAccount, crate::schema::auth::user_id_accounts::dsl::user_id_accounts);

#[cfg(test)]
mod test {
    use super::*;
    use subseq_util::tables::harness::{to_pg_db_name, DbHarness};
    use function_name::named;
    use crate::tables::test::MIGRATIONS;

    #[test]
    #[named]
    fn test_user_handle() {
        let db_name = to_pg_db_name(function_name!());
        let harness = DbHarness::new("localhost", "development", &db_name,
                                     Some(MIGRATIONS));
        let mut conn = harness.conn(); 
        let user = User::create(&mut conn, Uuid::new_v4(), "test@example.com", Some("test_user")).expect("user");
        let user2 = User::get(&mut conn, user.id).expect("user2");
        assert_eq!(user, user2);

        assert!(User::create(&mut conn, Uuid::new_v4(), "bad_user@example.com", Some("2bad_user")).is_err());
        assert!(User::create(&mut conn, Uuid::new_v4(), "bad_email", Some("bad_user")).is_err());
    }
}

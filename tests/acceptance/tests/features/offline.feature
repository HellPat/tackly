Feature: Tackly works without the server and syncs later

  Scenario: Patrick uses Tackly before any server exists, then shares it
    Given the sync server is stopped
    And Patrick, Mona and Mara have opened Tackly
    When Patrick creates the family "The Smiths"
    Then Patrick's app says it is offline
    When Patrick adds the task "Pack the bags"
    And the sync server is started
    Then Patrick's app says it is live
    When Patrick invites Mona
    And Mona shows All
    Then Mona sees the task "Pack the bags"

  Scenario: A server outage does not stop anyone, and all three catch up
    Given the sync server is running
    And Patrick, Mona and Mara have opened Tackly
    And Patrick has created the family "The Smiths" with Mona and Mara
    And Patrick has added the tasks "Laundry" and "Groceries"
    And Mona shows All
    And Mara shows All
    And Mona and Mara see the tasks "Laundry" and "Groceries"
    When the sync server is stopped
    Then Mona's app says it is offline
    When Mona starts "Laundry"
    And Mara ticks "Groceries" off
    And Patrick adds the task "Vacuum"
    And the sync server is started
    Then Mona and Mara see the task "Vacuum"
    And Patrick no longer sees "Groceries"
    And Mara sees that Mona is working on "Laundry"
